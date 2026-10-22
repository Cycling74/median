#!/bin/sh
# Formats staged Rust files with rustfmt before a commit. Installed by script/install-hooks.sh.
#
# Fully staged files are formatted in place and re-staged. Files that also have unstaged changes
# are only checked (re-staging them would sweep in the unstaged edits), and the commit is aborted
# if they need formatting. The editions come from the rustfmt.toml files (2024 at the root, 2018
# under max-sys/).
#
# Not formatted: the generated bindings (max-sys/src/ffi-*.rs, regenerate them instead) and the
# trybuild cases (median/tests/ui/*.rs, whose .stderr files record line numbers).

if ! command -v rustfmt > /dev/null 2>&1; then
  echo "rustfmt NOT installed or found in system path."
  echo "rustfmt is required for commits containing Rust files"
  echo "aborting…"
  exit 1
fi

STAGED_RS=$(git diff --cached --name-only --diff-filter=ACM \
  | grep '\.rs$' \
  | grep -v -E '^max-sys/src/ffi-.*\.rs$|^max-sys/thirdparty/|^median/tests/ui/' || true)
[ -z "${STAGED_RS}" ] && exit 0

CLEAN=""
PARTIAL=""
for f in ${STAGED_RS}; do
  if git diff --quiet -- "$f"; then
    CLEAN="${CLEAN} $f"
  else
    PARTIAL="${PARTIAL} $f"
  fi
done

if [ -n "${CLEAN}" ]; then
  RESULT=$(echo ${CLEAN} | xargs rustfmt 2>&1)
  STATUS=$?
  if [ ${STATUS} -ne 0 ]; then
    echo "⚠️  rustfmt:"
    printf "%s\n" "${RESULT}"
    echo "⚠️  Commit aborted."
    exit ${STATUS}
  fi
  REFORMATTED=$(git diff --name-only -- ${CLEAN})
  if [ -n "${REFORMATTED}" ]; then
    echo "${REFORMATTED}" | xargs git add
    echo "rustfmt: reformatted and re-staged:"
    printf "%s\n" "${REFORMATTED}" | sed 's/^/  /'
  fi
fi

if [ -n "${PARTIAL}" ]; then
  RESULT=$(echo ${PARTIAL} | xargs rustfmt --check 2>&1)
  STATUS=$?
  if [ ${STATUS} -ne 0 ]; then
    echo "⚠️  rustfmt (files with unstaged changes are not auto-formatted):"
    printf "%s\n" "${RESULT}"
    echo "⚠️  Commit aborted, reformat the necessary files and reattempt commit."
    exit ${STATUS}
  fi
fi
