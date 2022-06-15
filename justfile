run:
  cargo run --features parallel --example sandbox

trace:
  RUST_BACKTRACE=1 cargo run --features parallel --example sandbox

profile:
  cargo run --release --features parallel --features tracy --example sandbox

profile-single-thread:
  cargo run --release --features tracy --example sandbox

flamegraph:
  cargo flamegraph --features parallel --example sandbox

flamegraph-single-thread:
  cargo flamegraph --example sandbox

wasm:
  cargo run-wasm --example sandbox

# cargo-outdated only lists minor versions and above, but I like to update patch versions,
# so here's a silly way to list dependencies with new patch versions available
#
# list dependencies with new versions available (including patch versions)
outdated:
  for pkg in $(cargo metadata --no-deps --format-version 1 \
      | jq -c '.packages[0].dependencies[] | { name: .name, req: .req }'); do \
    name=$(echo $pkg | jq -r '.name'); \
    ver=$(echo $pkg | jq -r '.req'); \
    ver=${ver:1}; \
    cargo search $name | awk "/^$name = / { if (\$3 != \"\\\"$ver\\\"\") { print \$1 \$2 \$3 }}"; \
  done

