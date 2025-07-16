set shell := ["bash", "-cu"]

# Determine clipboard command
clipboard-cmd := if os() == "macos" {
    "pbcopy"
} else if os() == "linux" {
    "xclip -selection clipboard"
} else {
    "cat"
}

clippy:
    cargo clippy --all-targets -- -D warnings

copy-check:
    cargo check 2>&1 | {{clipboard-cmd}}

copy-test:
    cargo test 2>&1 | {{clipboard-cmd}}

lines:
    tokei
