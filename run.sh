#!/usr/bin/env bash
# Starts the leader + 3 followers together. Ctrl-C stops all of them.
set -e
cargo build --bin mini-db --bin follower   # build once, up front

trap 'kill 0' EXIT   # kill every process in this script's group on exit/Ctrl-C

./target/debug/follower 3001 &
./target/debug/follower 3002 &
./target/debug/follower 3003 &
./target/debug/mini-db &

wait   # block until Ctrl-C
