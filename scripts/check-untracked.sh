#!/bin/bash
# Pre-commit hook: fail if any .rs or .py files are untracked
#
# Usage: ./scripts/check-untracked.sh

set -e

# Find untracked .rs and .py files
untracked=$(git ls-files --others --exclude-standard -- '*.rs' '*.py')

if [ -n "$untracked" ]; then
    echo "Error: The following source files are not tracked by git:"
    echo
    echo "$untracked"
    echo
    echo "Please add them with 'git add' or add to .gitignore"
    exit 1
fi

exit 0

