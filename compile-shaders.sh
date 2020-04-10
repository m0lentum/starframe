#!/usr/bin/env sh

# compile the shaders separately from the project build to avoid
# build-time dependencies that would need to be installed by users
cd ./src/graphics/shaders && glslc -c ./*.{vert,frag}
