#!/bin/sh
# Install the git pre-commit hook, which runs script/pre-commit.sh (rustfmt on staged files), and
# point `git blame` at .git-blame-ignore-revs.
set -e
HOOKS="$(git rev-parse --git-path hooks)"
mkdir -p "${HOOKS}"
cat > "${HOOKS}/pre-commit" <<'HOOK'
#!/bin/sh
set -e
cd "$(git rev-parse --show-toplevel)"
if [ -x "./script/pre-commit.sh" ]; then
    eval "./script/pre-commit.sh"
fi
HOOK
chmod +x "${HOOKS}/pre-commit"
echo "installed ${HOOKS}/pre-commit"

# skip formatting-only commits in `git blame`
git config blame.ignoreRevsFile .git-blame-ignore-revs
echo "set blame.ignoreRevsFile"
