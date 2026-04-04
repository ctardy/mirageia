# Docker — Test MirageIA + Claude Code

## Build

```bash
cd docker
docker build -t mirageia-test .
```

## Lancer (mode interactif)

```bash
docker run -it -e ANTHROPIC_API_KEY=sk-ant-... mirageia-test
```

Le container :
1. Démarre le proxy MirageIA sur le port 3100
2. Vérifie que tout est OK (health check)
3. Ouvre un shell interactif

## Tester Claude Code via le proxy

Dans le container :

```bash
# Lancer Claude Code protégé par MirageIA
mirageia wrap -- claude

# Observer les requêtes dans un autre terminal
# docker exec -it <container_id> mirageia console
```

## Tester sans proxy (comparaison)

```bash
# Claude Code direct (sans proxy)
claude
```

## Vérification rapide (non interactif)

```bash
docker run --rm -e ANTHROPIC_API_KEY=sk-ant-... mirageia-test \
  bash -c 'curl -s http://localhost:3100/health && echo " OK"'
```
