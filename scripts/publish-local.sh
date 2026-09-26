#!/usr/bin/env bash
set -euo pipefail

project_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
openac_source="${OPENAC_SOURCE_DIR:-"${project_root}/../OpenAC"}"
host_version="$(python3 -c 'import json,sys; print(json.load(open(sys.argv[1]))["minHostVersion"])' "${project_root}/src/AcDream.Plugins.GoArrow/plugin.json")"
plugin_version="${PLUGIN_VERSION:-$(python3 - "${project_root}" <<'PY'
import hashlib, pathlib, re, subprocess, sys, xml.etree.ElementTree as ET
root = pathlib.Path(sys.argv[1])
def git(*args):
    return subprocess.check_output(["git", "-C", str(root), *args], text=True).strip()
try:
    tag = git("describe", "--tags", "--match", "v[0-9]*", "--abbrev=0")
except subprocess.CalledProcessError:
    tag = ""
base = tag[1:] if re.fullmatch(r"v\d+\.\d+\.\d+", tag) else ET.parse(root / "Directory.Build.props").findtext(".//Version")
changes = subprocess.check_output(["git", "-C", str(root), "diff", "HEAD"])
suffix = ".dirty." + hashlib.sha256(changes).hexdigest()[:8] if changes else ""
print(f"{base}-local.{git('rev-parse', '--short', 'HEAD')}{suffix}")
PY
)}"
host_tag="v${host_version}"

if ! git -C "${openac_source}" rev-parse --verify "refs/tags/${host_tag}" >/dev/null 2>&1; then
    echo "OpenAC tag ${host_tag} is unavailable in ${openac_source}." >&2
    exit 1
fi

host_checkout="$(mktemp -d)"
trap 'rm -rf "${host_checkout}"' EXIT
git -C "${openac_source}" archive "${host_tag}" | tar -x -C "${host_checkout}"

project="${project_root}/src/AcDream.Plugins.GoArrow/AcDream.Plugins.GoArrow.csproj"
output="${project_root}/dist/openac.goarrow"
dotnet restore "${project}" -p:OpenAcDir="${host_checkout}"
dotnet publish "${project}" --configuration Release --no-restore \
    -p:OpenAcDir="${host_checkout}" -p:Version="${plugin_version}" \
    -m:1 /nodeReuse:false --output "${output}"
python3 - "${output}/plugin.json" "${plugin_version}" <<'PY'
import json, pathlib, sys
path = pathlib.Path(sys.argv[1])
manifest = json.loads(path.read_text())
manifest["version"] = sys.argv[2]
path.write_text(json.dumps(manifest, indent=2) + "\n")
PY
echo "Published ${output} version ${plugin_version} against OpenAC ${host_version}."
