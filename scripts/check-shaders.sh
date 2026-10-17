#!/usr/bin/env bash
set -euo pipefail

project_dir=$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)
material_file="$project_dir/src/render3d/material.rs"

if ! command -v glslangValidator >/dev/null 2>&1; then
    echo "shader check: glslangValidator is required but was not found in PATH" >&2
    exit 127
fi
if [[ ! -f "$material_file" ]]; then
    echo "shader check: source file not found: $material_file" >&2
    exit 1
fi

extract_shader() {
    local name=$1
    awk -v name="$name" '
        index($0, "pub const " name ": &str = r#\"") == 1 {
            line = $0
            sub("^pub const " name ": &str = r#\"", "", line)
            print line
            in_shader = 1
            next
        }
        in_shader && /"#;$/ {
            line = $0
            sub(/"#;$/, "", line)
            print line
            found = 1
            exit
        }
        in_shader { print }
        END {
            if (!found) {
                exit 1
            }
        }
    ' "$material_file"
}

check_shader() {
    local name=$1
    local stage=$2
    if ! extract_shader "$name" | glslangValidator --stdin -S "$stage"; then
        echo "shader check failed: $name ($stage)" >&2
        return 1
    fi
}

check_shader MATERIAL_VERTEX_SHADER vert
check_shader MATERIAL_FRAGMENT_SHADER frag
check_shader SHADOW_VERTEX_SHADER vert
check_shader SHADOW_FRAGMENT_SHADER frag
echo "shader check: PASS (4 GLSL 100 shaders)"
