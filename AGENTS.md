# Instructions pour les agents IA

- **Créer ou modifier un outil** : suis `DEVELOPING.md` (section 3 « Instructions pour une IA »), avec `ktrx-dev new`, `ktrx-dev check` puis `ktrx-dev publish`. Ne publie rien tant que `check` n'est pas entièrement vert.
- Un outil = un projet Rust indépendant sous `tools/<MYTHE> ( <Nom> )/`, cible `wasm32-wasip2`. N'ajoute jamais de logique propre à un outil dans `engine/`.
- Une version publiée ne change jamais : incrémente `version` dans `manifest.toml` et `Cargo.toml` à chaque modification de l'archive.
- Ne mets jamais le mot de passe de l'équipe dans un fichier ou un commit. Demande-le à l'utilisateur au moment de publier, puis `ktrx-dev login --password …` (une fois par machine). Tout le reste (installer, lire le guide, `new`, `check`) se fait sans mot de passe.
- Ne commite pas `target/`, `module.wasm`, `node_modules/`.
