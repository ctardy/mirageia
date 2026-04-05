FROM ubuntu:24.04

ENV DEBIAN_FRONTEND=noninteractive

RUN apt-get update && apt-get install -y \
    curl \
    ca-certificates \
    docker.io \
    && rm -rf /var/lib/apt/lists/*

# Node.js 22 + Claude Code
RUN curl -fsSL https://deb.nodesource.com/setup_22.x | bash - \
    && apt-get install -y nodejs \
    && rm -rf /var/lib/apt/lists/*

RUN npm install -g @anthropic-ai/claude-code

# ttyd — terminal web
RUN curl -sSfL https://github.com/tsl0922/ttyd/releases/latest/download/ttyd.x86_64 \
    -o /usr/local/bin/ttyd \
    && chmod +x /usr/local/bin/ttyd

# Dossier utilisateur pour les binaires installés manuellement
RUN mkdir -p /root/.local/bin
ENV PATH="/root/.local/bin:$PATH"

WORKDIR /workspace

COPY docker/entrypoint.sh /entrypoint.sh
RUN chmod +x /entrypoint.sh

EXPOSE 7681

ENTRYPOINT ["/entrypoint.sh"]
