#!/bin/zsh
# Runs Blender 4.5 headless. Set BLENDER to override the search.
for b in "${BLENDER:-}" /Applications/Blender.app/Contents/MacOS/Blender ~/code/aldermoor-wild/.tools/Blender.app/Contents/MacOS/Blender; do
  [ -n "$b" ] && [ -x "$b" ] && exec "$b" -b --factory-startup "$@"
done
echo "Blender not found: install Blender 4.5 or set BLENDER" >&2; exit 1
