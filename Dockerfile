FROM rust:1.95-bookworm AS build
RUN rustup target add wasm32-unknown-unknown \
    && cargo install trunk --locked --version 0.21.14
WORKDIR /workspace
COPY Cargo.toml ./
COPY app ./app
COPY server ./server
COPY shared ./shared
RUN cargo build --release -p cabo-server
RUN cd app && trunk build --release

FROM debian:bookworm-slim
ENV PORT=8080 CABO_APP_DIR=/opt/cabo/app
COPY --from=build /workspace/target/release/cabo-server /usr/local/bin/cabo-server
COPY --from=build /workspace/app/dist/ /opt/cabo/app/
USER 65534:65534
CMD ["cabo-server"]
