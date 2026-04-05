#!/bin/bash
set -e

echo ""
echo "  ╔══════════════════════════════════════════╗"
echo "  ║  MirageIA — Environnement de test        ║"
echo "  ╚══════════════════════════════════════════╝"
echo ""

# Préparer les répertoires de persistance (nécessaire avec cap_drop:ALL — sans CAP_DAC_OVERRIDE)
mkdir -p /root/.claude /root/.local/share/keyrings || true
chmod 700 /root/.claude /root/.local/share/keyrings 2>/dev/null || true

# Télécharger la dernière version de MirageIA à chaque démarrage
echo "  → Téléchargement de la dernière version MirageIA..."
curl -sSfL https://github.com/ctardy/mirageia/releases/latest/download/mirageia-linux-x86_64.tar.gz \
    | tar xz -C /usr/local/bin/ \
    && chmod +x /usr/local/bin/mirageia

echo "  ✓ MirageIA $(mirageia --version)"
echo "  ✓ Claude Code $(claude --version 2>&1 | head -1)"

# Clé API : optionnelle (on peut se connecter via 'claude login' à l'intérieur)
if [ -n "$ANTHROPIC_API_KEY" ]; then
    echo "  ✓ ANTHROPIC_API_KEY détectée"
else
    echo "  ℹ  Pas de clé API — utilise 'claude login' pour te connecter"
fi

echo ""

# Lancer MirageIA en arrière-plan
echo "  → Démarrage du proxy MirageIA..."
mirageia &
sleep 1

if curl -sf http://localhost:3100/health > /dev/null 2>&1; then
    echo "  ✓ Proxy actif sur http://localhost:3100"
else
    echo "  ✗ Le proxy n'a pas démarré"
    exit 1
fi

echo ""
echo "  ─────────────────────────────────────────────────"
echo "  Terminal web disponible sur /mirageia/"
echo ""
echo "  Commandes disponibles :"
echo "    mirageia wrap -- claude    Lance Claude Code via le proxy"
echo "    claude                     Lance Claude Code directement"
echo "    mirageia console           Monitoring temps réel"
echo "    curl localhost:3100/health Health check"
echo "  ─────────────────────────────────────────────────"
echo ""

# Claude Code stocke les tokens OAuth dans ~/.claude/.credentials.json (Linux, sans keychain)
# Pas besoin de gnome-keyring ni dbus
exec ttyd --port 7681 --base-path /mirageia --writable --ping-interval 30 bash -c 'while true; do bash --login; done'
