# K~MAAT — Entropy-Meter
> Maât, déesse égyptienne de la mesure et de l'équilibre, qui pesait le cœur des défunts : cet outil pèse le caractère aléatoire d'un texte.

## Présentation
K~MAAT calcule l'**entropie de Shannon** d'un texte : le nombre moyen de bits d'information par symbole. Un texte très répétitif a une entropie proche de 0 ; un texte où tous les symboles sont aussi probables atteint le maximum. C'est une première observation utile pour auditer un mot de passe, un jeton ou une clé trouvés dans un fichier : une chaîne aléatoire de 32 caractères hexadécimaux approche 4 bits par symbole, un mot courant bien moins.

L'outil fait du pur calcul sur le texte reçu. Il n'ouvre aucun fichier, n'utilise pas le réseau et ne conserve rien.

## Utilisation
```sh
ktrx install maat
ktrx run maat "correct horse battery staple"
ktrx run maat "é€ü" --bytes
```
Par défaut, les symboles sont les caractères Unicode ; avec `--bytes`, ce sont les octets UTF-8 (un caractère accentué en compte alors plusieurs). Les options sont détaillées dans l'onglet « Utilisation ».

## Sortie
```json
{
  "unit": "characters",
  "length": 16,
  "unique_symbols": 16,
  "entropy_bits_per_symbol": 4.0,
  "max_entropy_bits_per_symbol": 4.0,
  "evenness": 1.0,
  "total_entropy_bits": 64.0,
  "classes": { "lowercase": 6, "uppercase": 0, "digits": 10, "symbols": 0, "whitespace": 0, "other": 0 }
}
```

- `entropy_bits_per_symbol` : entropie de Shannon, en bits par symbole.
- `max_entropy_bits_per_symbol` : le maximum possible pour ce nombre de symboles distincts (`log2` du nombre de symboles uniques).
- `evenness` : rapport des deux, entre 0 et 1. Près de 1, les symboles présents sont équiprobables.
- `total_entropy_bits` : entropie par symbole multipliée par la longueur.
- `classes` : nombre de minuscules, majuscules, chiffres, symboles ASCII, espaces et autres caractères.

## Limites
- L'entropie de Shannon décrit la **répartition des symboles du texte reçu**, pas la façon dont il a été produit. Un mot de passe aléatoire court peut afficher une entropie basse, et une phrase connue en afficher une élevée : le résultat est une observation, pas un verdict sur la solidité d'un secret.
- Le texte est limité à 1 Mio.
- Un texte vide est refusé.

## Permissions
Aucune : l'outil s'exécute entièrement isolé.
