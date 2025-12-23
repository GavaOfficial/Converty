# Stage 1: Build
FROM rust:slim AS builder

WORKDIR /app

# Installa dipendenze di sistema per build (incluso SQLite e curl per swagger-ui)
RUN apt-get update && apt-get install -y \
    pkg-config \
    libssl-dev \
    libsqlite3-dev \
    curl \
    && rm -rf /var/lib/apt/lists/*

# Copia i file del progetto
COPY Cargo.toml Cargo.lock* ./
COPY src ./src

# Compila in release senza Google Auth (per Docker deployment)
RUN cargo build --release --no-default-features

# Stage 2: Runtime (usa stessa base di rust:slim per compatibilità GLIBC)
FROM debian:trixie-slim

WORKDIR /app

# Installa FFmpeg, Poppler (pdftoppm), SQLite e curl per runtime
RUN apt-get update && apt-get install -y \
    ffmpeg \
    poppler-utils \
    ca-certificates \
    libsqlite3-0 \
    curl \
    && rm -rf /var/lib/apt/lists/*

# Crea directory per database
RUN mkdir -p /app/data

# Copia il binario compilato
COPY --from=builder /app/target/release/converty /app/converty

# Esponi la porta
EXPOSE 3000

# Variabili d'ambiente
ENV CONVERTY_HOST=0.0.0.0
ENV CONVERTY_PORT=3000
ENV RUST_LOG=info
ENV DATABASE_URL=sqlite:/app/data/converty.db?mode=rwc

# Volume per persistenza database
VOLUME ["/app/data"]

# Avvia l'API
CMD ["./converty"]
