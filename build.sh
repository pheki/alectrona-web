#!/bin/sh

# Enables command echoes
set -ex

wasm-pack build --target web
rm pkg/package.json
rm pkg/.gitignore
cp -a pkg/ built/
cp -a static/ built/
