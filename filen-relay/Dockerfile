# must be built from the filen-rs repository root
# cd .. && docker build -f filen-relay/Dockerfile . -t filen-relay && cd filen-relay

# see https://dioxuslabs.com/learn/0.7/tutorial/deploy#building-a-dockerfile
# using nightly-2026-02-20 toolchain as per rust-toolchain.toml

# build from the latest filen-rclone source, so the latest fixes are included
# todo: this is hacky and should be changed for the future
FROM golang:latest AS go-builder
RUN git clone https://github.com/FilenCloudDienste/filen-rclone.git /app
WORKDIR /app
RUN git switch upstream
RUN go get github.com/FilenCloudDienste/filen-sdk-go@latest
RUN go build && mv ./rclone ./rclone-v1.70.0-filen.14-linux-amd64

FROM rust:1 AS chef
RUN cargo +nightly-2026-02-20 install cargo-chef
WORKDIR /app

FROM chef AS planner
COPY . .
RUN cargo +nightly-2026-02-20 chef prepare --recipe-path recipe.json

FROM chef AS builder
COPY --from=planner /app/recipe.json recipe.json
RUN cargo +nightly-2026-02-20 chef cook --release --recipe-path recipe.json
RUN rustup target add wasm32-unknown-unknown
# install dioxus-cli
RUN curl -L --proto '=https' --tlsv1.2 -sSf https://raw.githubusercontent.com/cargo-bins/cargo-binstall/main/install-from-binstall-release.sh | bash
RUN cargo binstall dioxus-cli@0.7.3 --root /.cargo -y --force
ENV PATH="/.cargo/bin:$PATH"

COPY . .
RUN cd filen-relay && dx bundle --web --release

FROM debian:trixie-slim AS runtime
RUN apt-get update && apt-get install -y ca-certificates
ENV PORT=80
ENV IP=0.0.0.0
EXPOSE 80
WORKDIR /usr/local/app
# ADD https://github.com/FilenCloudDienste/filen-rclone/releases/download/v1.70.0-filen.14/rclone-v1.70.0-filen.14-linux-amd64 /usr/local/app/rclone_configs/
COPY --from=go-builder /app/rclone-v1.70.0-filen.14-linux-amd64 /usr/local/app/rclone_configs/
RUN chmod +x /usr/local/app/rclone_configs/rclone-v1.70.0-filen.14-linux-amd64
COPY --from=builder /app/target/dx/filen-relay/release/web/ /usr/local/app
ENTRYPOINT [ "/usr/local/app/filen-relay" ]