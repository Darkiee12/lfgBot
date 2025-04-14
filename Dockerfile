# Define build arguments
ARG DATABASE_URL="postgres://postgres:password@localhost:5432/example_so_sqlx_not_be_pissed"
ARG BRAWL_STARS_TOKEN="token"
ARG DISCORD_TOKEN="token"

FROM clux/muslrust:stable AS chef
USER root
RUN cargo install cargo-chef
WORKDIR /bot

FROM chef AS planner
COPY . .
RUN cargo chef prepare --recipe-path recipe.json

# Stage for building the project
FROM chef AS builder
ARG DATABASE_URL
ENV DATABASE_URL=$DATABASE_URL
ENV SQLX_OFFLINE=true

COPY --from=planner /bot/recipe.json recipe.json
RUN cargo chef cook --release --target x86_64-unknown-linux-musl --recipe-path recipe.json
COPY . .
RUN cargo build --release --target x86_64-unknown-linux-musl

# Final stage for the runtime image
FROM gcr.io/distroless/cc-debian12 AS runtime

# RUN apk update && apk add --no-cache \
#     ca-certificates \
#     && update-ca-certificates
WORKDIR /bot
COPY --from=builder /bot/target/x86_64-unknown-linux-musl/release/bot /usr/local/bin/bot
COPY .sqlx /bot/.sqlx
ENTRYPOINT ["/usr/local/bin/bot"]