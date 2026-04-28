#!/bin/bash
cargo test -p seer &> out.txt || true
grep -A 1 "failures:" out.txt
