/*
 *  Copyright (c) 2016-present, Facebook, Inc.
 *  All rights reserved.
 *
 *  This source code is licensed under the BSD-style license found in the
 *  LICENSE file in the root directory of this source tree. An additional grant
 *  of patent rights can be found in the PATENTS file in the same directory.
 *
 */
#include "eden/fs/fuse/Dispatcher.h"

#include <folly/Exception.h>
#include <folly/Format.h>
#include <folly/MoveWrapper.h>
#include <folly/executors/GlobalExecutor.h>
#include <folly/experimental/logging/xlog.h>

#include "eden/fs/fuse/DirHandle.h"
#include "eden/fs/fuse/FileHandle.h"
#include "eden/fs/fuse/RequestData.h"

using namespace folly;
using namespace std::chrono;

namespace facebook {
namespace eden {
namespace fusell {

Dispatcher::Attr::Attr(const struct stat& st, uint64_t timeout)
    : st(st), timeout_seconds(timeout) {}

fuse_attr_out Dispatcher::Attr::asFuseAttr() const {
  fuse_attr_out result;

  result.attr.ino = st.st_ino;
  result.attr.size = st.st_size;
  result.attr.blocks = st.st_blocks;
  result.attr.atime = st.st_atime;
  result.attr.atimensec = st.st_atim.tv_nsec;
  result.attr.mtime = st.st_mtime;
  result.attr.mtimensec = st.st_mtim.tv_nsec;
  result.attr.ctime = st.st_ctime;
  result.attr.ctimensec = st.st_ctim.tv_nsec;
  result.attr.mode = st.st_mode;
  result.attr.nlink = st.st_nlink;
  result.attr.uid = st.st_uid;
  result.attr.gid = st.st_gid;
  result.attr.rdev = st.st_rdev;
  result.attr.blksize = st.st_blksize;

  result.attr_valid_nsec = 0;
  result.attr_valid = timeout_seconds;

  return result;
}

Dispatcher::~Dispatcher() {}

Dispatcher::Dispatcher(ThreadLocalEdenStats* stats) : stats_(stats) {}

void Dispatcher::onConnectionReady() {}

FileHandleMap& Dispatcher::getFileHandles() {
  return fileHandles_;
}

std::shared_ptr<FileHandleBase> Dispatcher::getGenericFileHandle(uint64_t fh) {
  return fileHandles_.getGenericFileHandle(fh);
}

std::shared_ptr<FileHandle> Dispatcher::getFileHandle(uint64_t fh) {
  return fileHandles_.getFileHandle(fh);
}
std::shared_ptr<DirHandle> Dispatcher::getDirHandle(uint64_t dh) {
  return fileHandles_.getDirHandle(dh);
}

void Dispatcher::initConnection(const fuse_init_out& out) {
  connInfo_ = out;
  onConnectionReady();
}

void Dispatcher::destroy() {}

folly::Future<fuse_entry_out> Dispatcher::lookup(
    fusell::InodeNumber /*parent*/,
    PathComponentPiece /*name*/) {
  throwSystemErrorExplicit(ENOENT);
}

folly::Future<folly::Unit> Dispatcher::forget(
    fusell::InodeNumber /*ino*/,
    unsigned long /*nlookup*/) {
  return Unit{};
}

folly::Future<Dispatcher::Attr> Dispatcher::getattr(
    fusell::InodeNumber /*ino*/) {
  throwSystemErrorExplicit(ENOENT);
}

folly::Future<Dispatcher::Attr> Dispatcher::setattr(
    fusell::InodeNumber /*ino*/,
    const fuse_setattr_in& /*attr*/
) {
  FUSELL_NOT_IMPL();
}

folly::Future<std::string> Dispatcher::readlink(fusell::InodeNumber /*ino*/) {
  FUSELL_NOT_IMPL();
}

folly::Future<fuse_entry_out> Dispatcher::mknod(
    fusell::InodeNumber /*parent*/,
    PathComponentPiece /*name*/,
    mode_t /*mode*/,
    dev_t /*rdev*/) {
  FUSELL_NOT_IMPL();
}

folly::Future<fuse_entry_out>
Dispatcher::mkdir(fusell::InodeNumber, PathComponentPiece, mode_t) {
  FUSELL_NOT_IMPL();
}

folly::Future<folly::Unit> Dispatcher::unlink(
    fusell::InodeNumber,
    PathComponentPiece) {
  FUSELL_NOT_IMPL();
}

folly::Future<folly::Unit> Dispatcher::rmdir(
    fusell::InodeNumber,
    PathComponentPiece) {
  FUSELL_NOT_IMPL();
}

folly::Future<fuse_entry_out> Dispatcher::symlink(
    fusell::InodeNumber,
    PathComponentPiece,
    folly::StringPiece) {
  FUSELL_NOT_IMPL();
}

folly::Future<folly::Unit> Dispatcher::rename(
    fusell::InodeNumber,
    PathComponentPiece,
    fusell::InodeNumber,
    PathComponentPiece) {
  FUSELL_NOT_IMPL();
}

folly::Future<fuse_entry_out>
Dispatcher::link(fusell::InodeNumber, fusell::InodeNumber, PathComponentPiece) {
  FUSELL_NOT_IMPL();
}

folly::Future<std::shared_ptr<FileHandle>> Dispatcher::open(
    fusell::InodeNumber /*ino*/,
    int /*flags*/) {
  FUSELL_NOT_IMPL();
}

folly::Future<std::shared_ptr<DirHandle>> Dispatcher::opendir(
    fusell::InodeNumber /*ino*/,
    int /*flags*/) {
  FUSELL_NOT_IMPL();
}

folly::Future<struct fuse_kstatfs> Dispatcher::statfs(
    fusell::InodeNumber /*ino*/) {
  struct fuse_kstatfs info = {};

  // Suggest a large blocksize to software that looks at that kind of thing
  // bsize will be returned to applications that call pathconf() with
  // _PC_REC_MIN_XFER_SIZE
  info.bsize = getConnInfo().max_readahead;

  // The fragment size is returned as the _PC_REC_XFER_ALIGN and
  // _PC_ALLOC_SIZE_MIN pathconf() settings.
  // 4096 is commonly used by many filesystem types.
  info.frsize = 4096;

  // Ensure that namelen is set to a non-zero value.
  // The value we return here will be visible to programs that call pathconf()
  // with _PC_NAME_MAX.  Returning 0 will confuse programs that try to honor
  // this value.
  info.namelen = 255;

  return info;
}

folly::Future<folly::Unit> Dispatcher::setxattr(
    fusell::InodeNumber /*ino*/,
    folly::StringPiece /*name*/,
    folly::StringPiece /*value*/,
    int /*flags*/) {
  FUSELL_NOT_IMPL();
}

const int Dispatcher::kENOATTR =
#ifndef ENOATTR
    ENODATA // Linux
#else
    ENOATTR
#endif
    ;

folly::Future<std::string> Dispatcher::getxattr(
    fusell::InodeNumber /*ino*/,
    folly::StringPiece /*name*/) {
  throwSystemErrorExplicit(kENOATTR);
}

folly::Future<std::vector<std::string>> Dispatcher::listxattr(
    fusell::InodeNumber /*ino*/) {
  return std::vector<std::string>();
}

folly::Future<folly::Unit> Dispatcher::removexattr(
    fusell::InodeNumber /*ino*/,
    folly::StringPiece /*name*/) {
  FUSELL_NOT_IMPL();
}

folly::Future<folly::Unit> Dispatcher::access(
    fusell::InodeNumber /*ino*/,
    int /*mask*/) {
  // Note that if you mount with the "default_permissions" kernel mount option,
  // the kernel will perform all permissions checks for you, and will never
  // invoke access() directly.
  //
  // Implementing access() is only needed when not using the
  // "default_permissions" option.
  FUSELL_NOT_IMPL();
}

folly::Future<Dispatcher::Create>
Dispatcher::create(fusell::InodeNumber, PathComponentPiece, mode_t, int) {
  FUSELL_NOT_IMPL();
}

folly::Future<uint64_t> Dispatcher::bmap(
    fusell::InodeNumber /*ino*/,
    size_t /*blocksize*/,
    uint64_t /*idx*/) {
  FUSELL_NOT_IMPL();
}

const fuse_init_out& Dispatcher::getConnInfo() const {
  return connInfo_;
}

ThreadLocalEdenStats* Dispatcher::getStats() const {
  return stats_;
}
} // namespace fusell
} // namespace eden
} // namespace facebook
