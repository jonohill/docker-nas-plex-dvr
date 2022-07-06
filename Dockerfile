FROM rust:1.62.0 AS dvr-manager-build

WORKDIR /usr/src/app
COPY dvr-manager .
RUN cargo install --path .

FROM jonoh/nas-plex:v0.0.75

COPY --from=dvr-manager-build /usr/local/cargo/bin/dvr-manager /usr/local/bin/dvr-manager

COPY root/ /
