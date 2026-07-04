# Build stage
FROM rust:1-slim AS build
WORKDIR /app
COPY Cargo.toml ./
COPY Cargo.lock ./
COPY src ./src
RUN cargo build --release

# Runtime stage
FROM debian:bookworm-slim
RUN apt-get update \
    && apt-get install -y --no-install-recommends ca-certificates \
    && rm -rf /var/lib/apt/lists/*
COPY --from=build /app/target/release/report-proxy /usr/local/bin/report-proxy
EXPOSE 8091
ENV BIND_ADDR=0.0.0.0:8091
CMD ["report-proxy"]
