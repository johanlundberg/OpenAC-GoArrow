.PHONY: release min-host-version

# Usage: make min-host-version v=0.1.15
min-host-version:
	@test -n "$(v)" || (echo "usage: make min-host-version v=MAJOR.MINOR.PATCH" >&2; exit 2)
	@printf '%s\n' '$(v)' | grep -Eq '^[0-9]+\.[0-9]+\.[0-9]+$$' || \
		(echo "invalid host version: $(v) (expected MAJOR.MINOR.PATCH)" >&2; exit 2)
	@python3 -c 'import json, pathlib, sys; p=pathlib.Path("src/AcDream.Plugins.GoArrow/plugin.json"); d=json.loads(p.read_text()); d["minHostVersion"]=sys.argv[1]; p.write_text(json.dumps(d, indent=2)+"\n"); print(f"Updated {p} minHostVersion to {sys.argv[1]}")' "$(v)"
	@sed -i -E 's/^  OPENAC_TAG: ".*"$$/  OPENAC_TAG: "v$(v)"/' .github/workflows/ci-release.yml
	@grep -q '^  OPENAC_TAG: "v$(v)"$$' .github/workflows/ci-release.yml || \
		(echo "failed to update OPENAC_TAG" >&2; exit 1)
	@echo "Updated .github/workflows/ci-release.yml OPENAC_TAG to v$(v)"

# Usage: make release v=1.0.3
# The GitHub Actions release workflow runs when the version tag is pushed.
release:
	@test -n "$(v)" || (echo "usage: make release v=MAJOR.MINOR.PATCH" >&2; exit 2)
	@printf '%s\n' '$(v)' | grep -Eq '^[0-9]+\.[0-9]+\.[0-9]+$$' || \
		(echo "invalid version: $(v) (expected MAJOR.MINOR.PATCH)" >&2; exit 2)
	@test -z "$$(git status --porcelain)" || \
		(echo "working tree is not clean; commit or stash changes first" >&2; git status --short; exit 1)
	@tag="v$(v)"; \
		if git rev-parse --verify --quiet "refs/tags/$$tag" >/dev/null; then \
			echo "tag already exists locally: $$tag" >&2; exit 1; \
		fi; \
		if git ls-remote --exit-code --tags origin "refs/tags/$$tag" >/dev/null 2>&1; then \
			echo "tag already exists on origin: $$tag" >&2; exit 1; \
		fi; \
		git tag -a "$$tag" -m "Release $$tag"; \
		git push origin "$$tag"
