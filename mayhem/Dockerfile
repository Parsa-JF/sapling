FROM rustlang/rust:nightly as builder

RUN apt-get update && apt-get upgrade -y && \
    apt-get install -y build-essential clang-11 llvm-11-tools cmake clang curl binutils-dev libbfd-dev libunwind8-dev libblocksruntime-dev pkg-config libudev-dev libssl-dev && \
    ln -s /usr/bin/llvm-config-11 /usr/bin/llvm-config

RUN rustup component add rust-src
RUN cargo install -f cargo-fuzz

ADD . /sapling
WORKDIR /sapling

RUN cd ./eden/scm/lib/dag/fuzz/ && cargo +nightly fuzz build

# Move Fuzzers to a clean image
FROM ubuntu:20.04

COPY --from=builder sapling/eden/scm/lib/dag/fuzz/target/x86_64-unknown-linux-gnu/release/gca_octopus /
COPY --from=builder sapling/eden/scm/lib/dag/fuzz/target/x86_64-unknown-linux-gnu/release/gca_small /
COPY --from=builder sapling/eden/scm/lib/dag/fuzz/target/x86_64-unknown-linux-gnu/release/gca /
COPY --from=builder sapling/eden/scm/lib/dag/fuzz/target/x86_64-unknown-linux-gnu/release/range_medium /
COPY --from=builder sapling/eden/scm/lib/dag/fuzz/target/x86_64-unknown-linux-gnu/release/range_octopus /
COPY --from=builder sapling/eden/scm/lib/dag/fuzz/target/x86_64-unknown-linux-gnu/release/range_small /
COPY --from=builder sapling/eden/scm/lib/dag/fuzz/target/x86_64-unknown-linux-gnu/release/range /
