FROM ubuntu:24.04

# Éviter les prompts interactifs
ENV DEBIAN_FRONTEND=noninteractive

# Dépendances système
RUN apt-get update && apt-get install -y \
    curl \
    ca-certificates \
    gnupg \
    && rm -rf /var/lib/apt/lists/*

# Node.js 22 (pour Claude Code)
RUN curl -fsSL https://deb.nodesource.com/setup_22.x | bash - \
    && apt-get install -y nodejs \
    && rm -rf /var/lib/apt/lists/*

# Claude Code
RUN npm install -g @anthropic-ai/claude-code

# MirageIA — binaire depuis GitHub Releases
ARG MIRAGEIA_VERSION=v0.1.0
RUN curl -sSfL "https://github.com/ctardy/mirageia/releases/download/${MIRAGEIA_VERSION}/mirageia-linux-x86_64.tar.gz" \
    | tar xz -C /usr/local/bin/ \
    && chmod +x /usr/local/bin/mirageia

# Vérification
RUN mirageia --version && claude --version

# Répertoire de travail
WORKDIR /workspace

# Script d'entrée
COPY entrypoint.sh /entrypoint.sh
RUN chmod +x /entrypoint.sh

EXPOSE 3100

ENTRYPOINT ["/entrypoint.sh"]
