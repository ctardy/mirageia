#!/bin/bash
set -e

echo ""
echo "  ╔══════════════════════════════════════════╗"
echo "  ║  MirageIA — Environnement de test        ║"
echo "  ╚══════════════════════════════════════════╝"
echo ""

# Vérifier la clé API
if [ -z "$ANTHROPIC_API_KEY" ]; then
    echo "  ✗ ANTHROPIC_API_KEY non définie"
    echo ""
    echo "  Relancez avec :"
    echo "    docker run -it -e ANTHROPIC_API_KEY=sk-... mirageia-test"
    echo ""
    exit 1
fi

echo "  ✓ ANTHROPIC_API_KEY détectée"
echo "  ✓ MirageIA $(mirageia --version)"
echo "  ✓ Claude Code $(claude --version 2>&1 | head -1)"
echo ""

# Lancer MirageIA en arrière-plan
echo "  → Démarrage du proxy MirageIA..."
mirageia &
PROXY_PID=$!
sleep 1

# Vérifier que le proxy tourne
if curl -sf http://localhost:3100/health > /dev/null 2>&1; then
    echo "  ✓ Proxy actif sur http://localhost:3100"
    curl -s http://localhost:3100/health | python3 -m json.tool 2>/dev/null || curl -s http://localhost:3100/health
    echo ""
else
    echo "  ✗ Le proxy n'a pas démarré"
    exit 1
fi

# Mode interactif ou commande
if [ $# -eq 0 ]; then
    echo "  ─────────────────────────────────────────────────"
    echo ""
    echo "  Commandes disponibles :"
    echo ""
    echo "    mirageia wrap -- claude    Lance Claude Code via le proxy"
    echo "    claude                     Lance Claude Code directement (sans proxy)"
    echo "    mirageia console           Monitoring temps réel"
    echo "    curl localhost:3100/health Health check"
    echo ""
    echo "  ─────────────────────────────────────────────────"
    echo ""
    exec bash
else
    exec "$@"
fi
