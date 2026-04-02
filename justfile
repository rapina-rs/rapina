# rapina task runner — install just: https://github.com/casey/just

# List available recipes
default:
    @just --list

# Run all checks (fmt, clippy, tests)
check:
    cargo fmt --all -- --check
    cargo clippy --all-targets --all-features -- -D warnings
    cargo test --locked --all

# Run tests only
test:
    cargo test --locked --all

# Run doc tests
test-doc:
    cargo test --locked --doc --all-features

# Format code
fmt:
    cargo fmt --all

# Bump versions, update docs/examples, open release PR
# Usage: just release 0.12.0
release VERSION:
    #!/usr/bin/env bash
    set -euo pipefail

    VERSION="{{VERSION}}"
    echo "Preparing release v${VERSION}..."

    MINOR="${VERSION%.*}"

    # Bump Cargo.toml package versions
    sed -i '' 's/^version = ".*"/version = "{{VERSION}}"/' rapina/Cargo.toml rapina-macros/Cargo.toml rapina-cli/Cargo.toml

    # Bump rapina-macros cross-reference in rapina/Cargo.toml
    sed -i '' 's/rapina-macros = { version = "[^"]*"/rapina-macros = { version = "{{VERSION}}"/' rapina/Cargo.toml

    # Bump version references in docs and examples
    find docs/ rapina/examples/ \( -name "*.md" -o -name "*.toml" \) \
        | xargs sed -i '' "s/rapina = { version = \"[0-9][^\"]*\"/rapina = { version = \"{{VERSION}}\"/g"

    # Regenerate Cargo.lock
    cargo check --workspace

    # Create branch, commit, push, open PR
    git checkout -b release_{{VERSION}}
    git add rapina/Cargo.toml rapina-macros/Cargo.toml rapina-cli/Cargo.toml Cargo.lock
    git add docs/ rapina/examples/
    git commit -m "Bump version to {{VERSION}}"
    git push origin release_{{VERSION}}
    gh pr create \
        --title "Release v{{VERSION}}" \
        --base main \
        --head release_{{VERSION}} \
        --body "Bumps all version references to {{VERSION}}. Merge this PR then push the \`v{{VERSION}}\` tag to trigger the release."

    echo ""
    echo "PR opened. Once merged, tag and push:"
    echo "  git tag v{{VERSION}} && git push origin v{{VERSION}}"

# Push the release tag after the version bump PR is merged
# Usage: just tag 0.12.0
tag VERSION:
    git checkout main
    git pull origin main
    git tag v{{VERSION}}
    git push origin v{{VERSION}}
    echo "Tag v{{VERSION}} pushed — CI will handle the GitHub release and crates.io publish."
