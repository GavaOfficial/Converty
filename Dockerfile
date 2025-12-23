# =============================================================================
# Stage 1: Chef - Prepara cargo-chef per caching dipendenze
# =============================================================================
FROM rust:slim AS chef

RUN cargo install cargo-chef --locked
WORKDIR /app

# =============================================================================
# Stage 2: Planner - Genera il "recipe" delle dipendenze
# =============================================================================
FROM chef AS planner

COPY Cargo.toml Cargo.lock* ./
COPY src ./src

# Genera recipe.json (cattura tutte le dipendenze)
RUN cargo chef prepare --recipe-path recipe.json

# =============================================================================
# Stage 3: Builder - Compila dipendenze (CACHED) e poi il progetto
# =============================================================================
FROM chef AS builder

# Installa dipendenze di sistema per build
RUN apt-get update && apt-get install -y \
    pkg-config \
    libssl-dev \
    libsqlite3-dev \
    curl \
    && rm -rf /var/lib/apt/lists/*

# Copia recipe e compila SOLO le dipendenze (questo layer viene cachato!)
COPY --from=planner /app/recipe.json recipe.json
RUN cargo chef cook --release --no-default-features --recipe-path recipe.json

# Ora copia il codice sorgente e compila l'applicazione
COPY Cargo.toml Cargo.lock* ./
COPY src ./src

# Build finale (le dipendenze sono già compilate e cachate)
RUN cargo build --release --no-default-features

# =============================================================================
# Stage 4: Runtime - Immagine finale minimale
# =============================================================================
FROM debian:trixie-slim

WORKDIR /app

# Installa solo le dipendenze runtime necessarie
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
