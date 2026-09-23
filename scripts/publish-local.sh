#!/usr/bin/env bash
set -euo pipefail

project_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
openac_source="${OPENAC_SOURCE_DIR:-"${project_root}/../OpenAC"}"
host_version="$(python3 -c 'import json,sys; print(json.load(open(sys.argv[1]))["minHostVersion"])' "${project_root}/src/AcDream.Plugins.GoArrow/plugin.json")"
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
    -p:OpenAcDir="${host_checkout}" -m:1 /nodeReuse:false --output "${output}"
echo "Published ${output} against OpenAC ${host_version}."
