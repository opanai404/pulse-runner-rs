FROM rust:1.94.0-slim-bookworm AS build

WORKDIR /app
COPY . .
RUN cargo build --release

FROM debian:bookworm-slim

RUN useradd --system --user-group --home-dir /app pulse
WORKDIR /app
COPY --from=build /app/target/release/pulse-runner-rs /usr/local/bin/pulse-runner-rs
COPY --from=build /app/assets ./assets
USER pulse

ENV PULSE_RUNNER_ADDR=0.0.0.0:8080
EXPOSE 8080

CMD ["pulse-runner-rs"]
