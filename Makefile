.PHONY: release

# Usage: make release v=0.1.1
# The GitHub Actions release workflow runs when the version tag is pushed.
# Bump "version" in plugin.json and commit it first: the workflow publishes
# plugin.json as it is and refuses a tag that disagrees with it.
release:
	@test -n "$(v)" || (echo "usage: make release v=MAJOR.MINOR.PATCH" >&2; exit 2)
	@printf '%s\n' '$(v)' | grep -Eq '^[0-9]+\.[0-9]+\.[0-9]+$$' || \
		(echo "invalid version: $(v) (expected MAJOR.MINOR.PATCH)" >&2; exit 2)
	@manifest="$$(python3 -c 'import json; print(json.load(open("plugin.json"))["version"])')"; \
		test "$$manifest" = "$(v)" || \
		(echo "plugin.json version is $$manifest, not $(v); bump it and commit first" >&2; exit 1)
	@test -z "$$(git status --porcelain)" || \
		(echo "working tree is not clean; commit or stash changes first" >&2; git status --short; exit 1)
	@tag="wasm-v$(v)"; \
		branch="$$(git branch --show-current)"; \
		if [ -z "$$branch" ]; then \
			echo "release must run from a branch" >&2; exit 1; \
		fi; \
		if git rev-parse --verify --quiet "refs/tags/$$tag" >/dev/null; then \
			echo "tag already exists locally: $$tag" >&2; exit 1; \
		fi; \
		if git ls-remote --exit-code --tags origin "refs/tags/$$tag" >/dev/null 2>&1; then \
			echo "tag already exists on origin: $$tag" >&2; exit 1; \
		fi; \
		git tag -a "$$tag" -m "Release $$tag"; \
		git push origin "HEAD:$$branch"; \
		git push origin "$$tag"
