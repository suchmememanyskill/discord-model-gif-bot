FROM rust:1.88.0-slim AS build
WORKDIR /source
COPY . .
RUN apt update && apt install -y wget xz-utils
RUN cargo build --release
RUN wget -O gifski.tar.xz https://github.com/ImageOptim/gifski/releases/download/1.34.0/gifski-1.34.0.tar.xz
RUN tar -xvf gifski.tar.xz

FROM debian:stable-slim as runtime
WORKDIR /app
COPY --from=build /source/target/release/discord-model-gif-bot .
COPY --from=build /source/linux/gifski .
RUN chmod +x ./discord-model-gif-bot ./gifski

ENV GIFSKI_PATH=/app/gifski

ENTRYPOINT ["/app/discord-model-gif-bot"]