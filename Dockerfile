FROM rust:1.95-bookworm AS build
RUN rustup target add wasm32-unknown-unknown \
    && cargo install trunk --locked --version 0.21.14
WORKDIR /app
COPY Cargo.toml index.html ./
COPY src ./src
RUN trunk build --release

FROM nginx:1.28-alpine
ENV PORT=8080
COPY nginx.conf.template /etc/nginx/templates/default.conf.template
COPY --from=build /app/dist/ /usr/share/nginx/html/
