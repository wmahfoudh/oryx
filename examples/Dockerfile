# Build stage compiles the release binary.
FROM rust:1.80 AS build
WORKDIR /src
COPY . .
RUN cargo build --release

FROM debian:bookworm-slim
COPY --from=build /src/target/release/oryx /usr/local/bin/oryx
ENV ORYX_THEME=oryx-dark
ENTRYPOINT ["oryx"]
