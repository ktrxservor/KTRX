# Tech Fingerprint

Outil de reconnaissance technologique web inspiré de Wappalyzer, exécuté dans KTRX. Il combine les en-têtes HTTP, cookies, méta-tags, HTML, chemins d’assets, scripts JavaScript, CSS, liens same-origin, fichiers de découverte usuels, DNS et validation TLS. Il tente de relever le nom et la version quand ces indices sont présents, avec la preuve et un niveau de confiance.

## Capacités

- CMS et commerce : WordPress, Drupal, Joomla, Shopify, Magento, PrestaShop, Ghost, Wix, Squarespace et Webflow.
- Serveurs, plateformes et frameworks : nginx, Apache, IIS, Caddy, Cloudflare, Vercel, Netlify, PHP, ASP.NET, Django, Rails, Laravel, Spring, Express et autres signatures visibles.
- Front-end et assets : React, Vue, Angular, Next.js, Nuxt, Svelte, Gatsby, Astro, jQuery, Bootstrap, Tailwind, bibliothèques et CDN.
- Analytics, tags et services : Google Analytics/GTM, Matomo, Segment, Hotjar, Mixpanel, Plausible, Microsoft Clarity, Stripe, Auth0 et Firebase.
- Sources d’indices : valeurs de headers, cookies et attributs, generator meta, URLs, noms/version d’assets, et marqueurs visibles dans les échantillons de code.
- Navigation limitée aux liens de même origine ; suit scripts et styles et consulte des chemins usuels comme robots.txt, sitemap.xml, manifest.json, wp-json et security.txt.

Les dépendances externes sont recensées mais pas suivies par défaut. L’outil ne lance pas JavaScript et ne peut donc pas découvrir les technologies qui n’apparaissent qu’après exécution côté navigateur. L’API KTRX ne fournit pas les champs détaillés de certificat TLS ni les réponses DNS CNAME. Le rapport indique ces limites pour éviter de présenter une empreinte comme exhaustive.

## Utilisation

```sh
ktrx run tech-fingerprint.tool https://example.com --max-requests 50 --depth 2 --body-kb 256
```

KTRX demande l’accord réseau avant l’exécution. Le nombre de requêtes est borné à 150 ; les réponses restent limitées par le moteur à 1 Mio et l’échantillon remis à l’outil à 256 Kio par ressource. La progression affiche en direct le nombre de ressources inspectées et de technologies distinctes repérées.

## Construction

```sh
rustup target add wasm32-wasip2
cargo build --release --target wasm32-wasip2
cp target/wasm32-wasip2/release/ktrx_tech_fingerprint.wasm module.wasm
ktrx pack . --output tech-fingerprint.tool
ktrx validate tech-fingerprint.tool
```
