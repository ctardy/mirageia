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

# Charger le modèle ONNX PII si déjà en cache (persisté dans /root/.mirageia/models/)
# Le modèle doit être préparé manuellement (export ONNX depuis HuggingFace avec Optimum)
# → cd /tmp && pip install optimum && optimum-cli export onnx --model iiiorg/piiranha-v1-detect-personal-information --task token-classification piiranha_onnx/
# → mv piiranha_onnx/model.onnx piiranha_onnx/tokenizer.json piiranha_onnx/config.json ~/.mirageia/models/iiiorg__piiranha-v1-detect-personal-information/
# → mirageia model use iiiorg/piiranha-v1-detect-personal-information
ONNX_MODEL="iiiorg/piiranha-v1-detect-personal-information"
ONNX_MODEL_DIR="/root/.mirageia/models/$(echo "$ONNX_MODEL" | sed 's|/|__|g')"
if [ -f "$ONNX_MODEL_DIR/model.onnx" ]; then
    mirageia model use "$ONNX_MODEL"
    echo "  ✓ Modèle ONNX actif — détection contextuelle activée"
else
    echo "  ℹ  Modèle ONNX absent — détection regex seule (placez les fichiers ONNX dans $ONNX_MODEL_DIR)"
fi

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

# Wait up to 120s for the proxy to start (ONNX model loading can take 30-60s)
READY=0
for i in $(seq 1 120); do
    sleep 1
    if curl -sf http://localhost:3100/health > /dev/null 2>&1; then
        READY=1
        break
    fi
done

if [ "$READY" = "1" ]; then
    echo "  ✓ Proxy actif sur http://localhost:3100"
else
    echo "  ✗ Le proxy n'a pas démarré (timeout 120s)"
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
