/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#![feature(try_blocks)]

mod git_reader;
mod gitimport_objects;
mod gitlfs;

use std::collections::BTreeMap;
use std::collections::HashSet;
use std::path::Path;
use std::process::Stdio;
use std::sync::RwLock;

use anyhow::bail;
use anyhow::format_err;
use anyhow::Context;
use anyhow::Error;
use bytes::Bytes;
use cloned::cloned;
use context::CoreContext;
use futures::Stream;
use futures::StreamExt;
use futures::TryFutureExt;
use futures::TryStreamExt;
use git_hash::ObjectId;
use git_object::Object;
use linked_hash_map::LinkedHashMap;
use manifest::bonsai_diff;
use manifest::BonsaiDiffFileChange;
use mononoke_types::ChangesetId;
use mononoke_types::MPath;
use slog::debug;
use slog::info;
use sorted_vector_map::SortedVectorMap;
use tokio::io::AsyncBufReadExt;
use tokio::io::BufReader;
use tokio::process::Command;
use tokio::task;

pub use crate::git_reader::GitRepoReader;
pub use crate::gitimport_objects::convert_time_to_datetime;
pub use crate::gitimport_objects::oid_to_sha1;
use crate::gitimport_objects::read_raw_object;
pub use crate::gitimport_objects::CommitMetadata;
pub use crate::gitimport_objects::ExtractedCommit;
pub use crate::gitimport_objects::GitLeaf;
pub use crate::gitimport_objects::GitManifest;
pub use crate::gitimport_objects::GitTree;
pub use crate::gitimport_objects::GitUploader;
pub use crate::gitimport_objects::GitimportPreferences;
pub use crate::gitimport_objects::GitimportTarget;
pub use crate::gitlfs::GitImportLfs;
pub use crate::gitlfs::LfsMetaData;

pub const HGGIT_MARKER_EXTRA: &str = "hg-git-rename-source";
pub const HGGIT_MARKER_VALUE: &[u8] = b"git";
pub const HGGIT_COMMIT_ID_EXTRA: &str = "convert_revision";

// TODO: Try to produce copy-info?
async fn find_file_changes<S, U>(
    ctx: &CoreContext,
    lfs: &GitImportLfs,
    reader: &GitRepoReader,
    uploader: U,
    changes: S,
) -> Result<SortedVectorMap<MPath, U::Change>, Error>
where
    S: Stream<Item = Result<BonsaiDiffFileChange<GitLeaf>, Error>>,
    U: GitUploader,
{
    changes
        .map_ok(|change| async {
            task::spawn({
                cloned!(ctx, reader, uploader, lfs);
                async move {
                    match change {
                        BonsaiDiffFileChange::Changed(path, ty, GitLeaf(oid))
                        | BonsaiDiffFileChange::ChangedReusedId(path, ty, GitLeaf(oid)) => {
                            let git_bytes = {
                                let object = reader.get_object(&oid).await?;
                                let blob = object
                                    .parsed
                                    .try_into_blob()
                                    .map_err(|_| format_err!("{} is not a blob", oid))?;
                                Bytes::from(blob.data)
                            };

                            uploader
                                .upload_file(&ctx, &lfs, &path, ty, oid, git_bytes)
                                .await
                                .map(|change| (path, change))
                        }
                        BonsaiDiffFileChange::Deleted(path) => Ok((path, U::deleted())),
                    }
                }
            })
            .await?
        })
        .try_buffer_unordered(100)
        .try_collect()
        .await
}

pub struct GitimportAccumulator {
    inner: LinkedHashMap<ObjectId, ChangesetId>,
}

impl GitimportAccumulator {
    pub fn new() -> Self {
        Self {
            inner: LinkedHashMap::new(),
        }
    }

    pub fn len(&self) -> usize {
        self.inner.len()
    }

    pub fn is_empty(&self) -> bool {
        self.inner.is_empty()
    }

    pub fn insert(&mut self, oid: ObjectId, cs_id: ChangesetId) {
        self.inner.insert(oid, cs_id);
    }

    pub fn get(&self, oid: &git_hash::oid) -> Option<ChangesetId> {
        self.inner.get(oid).copied()
    }
}

pub async fn upload_git_tag<Uploader: GitUploader>(
    ctx: &CoreContext,
    uploader: &Uploader,
    path: &Path,
    prefs: &GitimportPreferences,
    tag_id: &ObjectId,
) -> Result<(), Error> {
    let reader = GitRepoReader::new(&prefs.git_command_path, path).await?;
    let tag_bytes = read_raw_object(&reader, tag_id)
        .await
        .with_context(|| format_err!("Failed to fetch git tag {}", tag_id))?;
    // Upload Git Tag
    uploader
        .upload_object(ctx, *tag_id, tag_bytes)
        .await
        .with_context(|| format_err!("Failed to upload raw git tag {}", tag_id,))
}

pub async fn gitimport_acc<Uploader: GitUploader>(
    ctx: &CoreContext,
    path: &Path,
    uploader: Uploader,
    target: &GitimportTarget,
    prefs: &GitimportPreferences,
) -> Result<GitimportAccumulator, Error> {
    let repo_name = if let Some(name) = &prefs.gitrepo_name {
        String::from(name)
    } else {
        let name_path = if path.ends_with(".git") {
            path.parent().unwrap_or(path)
        } else {
            path
        };
        String::from(name_path.to_string_lossy())
    };
    let dry_run = prefs.dry_run;

    let reader = GitRepoReader::new(&prefs.git_command_path, path).await?;
    let roots = target.get_roots();
    // TODO - remove this
    let nb_commits_to_import = target.get_nb_commits(&prefs.git_command_path, path).await?;
    if 0 == nb_commits_to_import {
        info!(ctx.logger(), "Nothing to import for repo {}.", repo_name);
        return Ok(GitimportAccumulator::new());
    }

    let acc = RwLock::new(GitimportAccumulator::new());

    // Kick off a stream that consumes the walk and prepared commits. Then, produce the Bonsais.
    target
        .list_commits(&prefs.git_command_path, path)
        .await?
        .try_filter_map({
            let acc = &acc;
            let uploader = &uploader;
            let repo_name = &repo_name;
            move |oid| async move {
                if let Some(bcs_id) = uploader.check_commit_uploaded(ctx, &oid).await? {
                    acc.write().expect("lock poisoned").insert(oid, bcs_id);
                    let git_sha1 = oid_to_sha1(&oid)?;
                    info!(
                        ctx.logger(),
                        "GitRepo:{} commit {} of {} - Oid:{} => Bid:{} (already exists)",
                        repo_name,
                        acc.read().expect("lock poisoned").len(),
                        nb_commits_to_import,
                        git_sha1.to_brief(),
                        bcs_id.to_brief()
                    );
                    Ok(None)
                } else {
                    Ok(Some(oid))
                }
            }
        })
        .map_ok(|oid| {
            cloned!(ctx, reader, uploader, prefs.lfs);
            async move {
                task::spawn({
                    async move {
                        let ExtractedCommit {
                            metadata,
                            tree,
                            parent_trees,
                            original_commit,
                        } = ExtractedCommit::new(&ctx, oid, &reader)
                            .await
                            .with_context(|| format!("While extracting {}", oid))?;

                        let file_changes = find_file_changes(
                            &ctx,
                            &lfs,
                            &reader,
                            uploader,
                            bonsai_diff(ctx.clone(), reader.clone(), tree, parent_trees),
                        )
                        .await?;

                        Result::<_, Error>::Ok((metadata, file_changes, original_commit, tree))
                    }
                })
                .await?
            }
        })
        .try_buffered(prefs.concurrency)
        .and_then(|(metadata, file_changes, original_commit, tree)| {
            let acc = &acc;
            let uploader = &uploader;
            let repo_name = &repo_name;
            let reader = &reader;
            async move {
                let oid = metadata.oid;
                let bonsai_parents = metadata
                    .parents
                    .iter()
                    .map(|p| {
                        roots
                            .get(p)
                            .copied()
                            .or_else(|| acc.read().expect("lock poisoned").get(p))
                            .ok_or_else(|| {
                                format_err!(
                                    "Couldn't find parent: {} in local list of imported commits",
                                    p
                                )
                            })
                    })
                    .collect::<Result<Vec<_>, _>>()
                    .with_context(|| format_err!("While looking for parents of {}", oid))?;

                // Before generating the corresponding changeset at Mononoke end, upload the raw git commit
                // and the git tree pointed to by the git commit.
                let tree_for_commit =
                    read_raw_object(reader, &tree.0).await.with_context(|| {
                        format_err!("Failed to fetch git tree {} for commit {}", tree.0, oid)
                    })?;
                // Upload Git Tree
                uploader
                    .upload_object(ctx, tree.0, tree_for_commit)
                    .await
                    .with_context(|| {
                        format_err!(
                            "Failed to upload raw git tree {} for commit {}",
                            tree.0,
                            oid
                        )
                    })?;
                // Upload Git commit
                uploader
                    .upload_object(ctx, oid, original_commit)
                    .await
                    .with_context(|| format_err!("Failed to upload raw git commit {}", oid))?;
                let (int_cs, bcs_id) = uploader
                    .generate_changeset(ctx, bonsai_parents, metadata, file_changes, dry_run)
                    .await?;
                acc.write().expect("lock poisoned").insert(oid, bcs_id);

                let git_sha1 = oid_to_sha1(&oid)?;
                info!(
                    ctx.logger(),
                    "GitRepo:{} commit {} of {} - Oid:{} => Bid:{}",
                    &repo_name,
                    acc.read().expect("lock poisoned").len(),
                    nb_commits_to_import,
                    git_sha1.to_brief(),
                    bcs_id.to_brief()
                );
                Ok((int_cs, git_sha1))
            }
        })
        // Chunk together into Vec<std::result::Result<(bcs, oid), Error> >
        .chunks(prefs.concurrency)
        // Go from Vec<Result<X,Y>> -> Result<Vec<X>,Y>
        .map(|v| v.into_iter().collect::<Result<Vec<_>, Error>>())
        .try_for_each(|v| async {
            cloned!(ctx, uploader);
            task::spawn(async move { uploader.finalize_batch(&ctx, dry_run, v).await }).await?
        })
        .await?;

    debug!(ctx.logger(), "Completed git import for repo {}.", repo_name);
    Ok(acc.into_inner().expect("lock poisoned"))
}

pub async fn gitimport(
    ctx: &CoreContext,
    path: &Path,
    uploader: impl GitUploader,
    target: &GitimportTarget,
    prefs: &GitimportPreferences,
) -> Result<LinkedHashMap<ObjectId, ChangesetId>, Error> {
    Ok(gitimport_acc(ctx, path, uploader, target, prefs)
        .await?
        .inner)
}

/// Object representing Git refs. maybe_tag_id will only
/// have a value if the ref is a tag pointing to a commit.
#[derive(Debug, Hash, PartialEq, Eq, PartialOrd, Ord)]
pub struct GitRef {
    pub name: Vec<u8>,
    pub maybe_tag_id: Option<ObjectId>,
}

impl GitRef {
    fn new(name: Vec<u8>) -> Self {
        Self {
            name,
            maybe_tag_id: None,
        }
    }
}

pub async fn read_git_refs(
    path: &Path,
    prefs: &GitimportPreferences,
) -> Result<BTreeMap<GitRef, ObjectId>, Error> {
    let reader = GitRepoReader::new(&prefs.git_command_path, path).await?;

    let mut command = Command::new(&prefs.git_command_path)
        .current_dir(path)
        .env_clear()
        .kill_on_drop(false)
        .stdout(Stdio::piped())
        .arg("for-each-ref")
        .arg("--format=%(objectname) %(refname)")
        .spawn()
        .with_context(|| format!("failed to run git with {:?}", prefs.git_command_path))?;
    let stdout = BufReader::new(command.stdout.take().context("stdout not set up")?);
    let mut lines = stdout.lines();

    let mut refs = BTreeMap::new();

    while let Some(line) = lines
        .next_line()
        .await
        .context("git command didn't output anything")?
    {
        if let Some((oid_str, ref_name)) = line.split_once(' ') {
            let mut oid: ObjectId = oid_str.parse().context("reading refs")?;
            let mut git_ref = GitRef::new(ref_name.into());
            loop {
                let object = reader.get_object(&oid).await.with_context(|| {
                    format!("unable to read git object: {oid} for ref: {ref_name}")
                })?;
                match object.parsed {
                    Object::Tree(_) => {
                        // This happens in the Linux kernel repo, because Linus was being clever - a commit and a tree
                        // are both treeish for the purposes of things like checkout and diff.
                        break;
                    }
                    Object::Blob(_) => {
                        bail!("ref {} points to a blob", ref_name);
                    }
                    Object::Commit(_) => {
                        refs.insert(git_ref, oid);
                        break;
                    }
                    // If the ref is a tag, then we capture the object id of the tag.
                    // The loop is designed to peel the tag but we want the outermost
                    // tag object so only get the ID if we haven't already done it before.
                    Object::Tag(tag) => {
                        if git_ref.maybe_tag_id.is_none() {
                            git_ref.maybe_tag_id = Some(oid);
                        }
                        oid = tag.target;
                    }
                }
            }
        }
    }
    Ok(refs)
}

pub async fn import_tree_as_single_bonsai_changeset(
    ctx: &CoreContext,
    path: &Path,
    uploader: impl GitUploader,
    git_cs_id: ObjectId,
    prefs: &GitimportPreferences,
) -> Result<ChangesetId, Error> {
    let reader = GitRepoReader::new(&prefs.git_command_path, path).await?;

    let sha1 = oid_to_sha1(&git_cs_id)?;

    let ExtractedCommit {
        tree,
        metadata,
        original_commit,
        ..
    } = ExtractedCommit::new(ctx, git_cs_id, &reader)
        .await
        .with_context(|| format!("While extracting {}", git_cs_id))?;

    let file_changes = find_file_changes(
        ctx,
        &prefs.lfs,
        &reader.clone(),
        uploader.clone(),
        bonsai_diff(ctx.clone(), reader, tree, HashSet::new()),
    )
    .await?;

    // Before generating the corresponding changeset at Mononoke end, upload the raw git commit.
    uploader
        .upload_object(ctx, git_cs_id, original_commit)
        .await
        .with_context(|| format_err!("Failed to upload raw git commit {}", git_cs_id))?;

    uploader
        .generate_changeset(ctx, vec![], metadata, file_changes, prefs.dry_run)
        .and_then(|(cs, id)| {
            uploader
                .finalize_batch(ctx, prefs.dry_run, vec![(cs, sha1)])
                .map_ok(move |_| id)
        })
        .await
}
