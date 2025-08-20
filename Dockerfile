FROM rust:1.88.0-slim AS build
WORKDIR /source
COPY . .
RUN apt update && apt install -y wget xz-utils
RUN cargo build --release
RUN wget -O mesh-thumbnail https://github.com/suchmememanyskill/mesh-thumbnail/releases/download/v1.6/mesh-thumbnail-x86_64-unknown-linux-gnu
RUN wget -O gifski.tar.xz https://github.com/ImageOptim/gifski/releases/download/1.34.0/gifski-1.34.0.tar.xz
RUN tar -xvf gifski.tar.xz

FROM debian:bookworm-slim as runtime
WORKDIR /app
COPY --from=build /source/target/release/discord-model-gif-bot .
COPY --from=build /source/mesh-thumbnail .
COPY --from=build /source/linux/gifski .
COPY start.sh .
RUN apt update && apt install -y libfreetype6 libfontconfig xvfb libxcursor-dev libxi-dev && apt-get clean && rm -rf /var/lib/apt/lists/*
RUN chmod +x ./discord-model-gif-bot ./mesh-thumbnail ./gifski ./start.sh

ENV GIFSKI_PATH=/app/gifski
ENV MESH_THUMBNAIL_PATH=/app/mesh-thumbnail

ENTRYPOINT ["/app/start.sh"]