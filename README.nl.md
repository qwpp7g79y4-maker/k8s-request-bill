# k8s-request-bill

Overprovisioning-rapport voor Kubernetes, offline. Het leest opgeslagen
`kubectl`-JSON (requests en limits van pods, en optioneel gebruik) en toont
**per namespace** wat is aangevraagd, wat is gebruikt en wat overblijft, in
**vCPU** en **GiB**, plus de **kosten tegen een nodetarief dat jij opgeeft**.

De Engelse README is `README.md`. Die is leidend; dit is de Nederlandse versie
met dezelfde inhoud. De tests en de gemeten aantallen staan in `QUALITY.nl.md`.

Het leest alleen bestanden (of stdin). Het maakt geen netwerkverbindingen, praat
nooit met een cluster, heeft geen kubeconfig nodig en bevat geen prijslijst: het
tarief is van jou.

## Installatie

Versie 0.1.0. Rust 1.85 of nieuwer. Vanuit een checkout:

```sh
cargo install --path .
```

Zodra de crate op crates.io staat, werkt ook `cargo install k8s-request-bill`.

## Snelstart

Sla de twee invoerbestanden één keer op (dit is de enige stap die een cluster
raakt, en die draai je zelf):

```sh
kubectl get pods -A -o json > pods.json
kubectl get --raw /apis/metrics.k8s.io/v1beta1/pods > metrics.json   # optioneel
```

Draai daarna de tool op die bestanden:

```sh
k8s-request-bill pods.json --usage metrics.json
```

## Aan de slag in 5 minuten

Je hebt geen cluster nodig om het uit te proberen. De repository bevat een klein
voorbeeld in `examples/`. De opdrachten hieronder lezen het alleen en schrijven
niets weg.

1. Bouw en installeer vanuit de checkout, zoals onder Installatie.
2. Alleen requests:

```sh
k8s-request-bill examples/pods.json
```

3. Voeg gebruik toe, om te zien wat overblijft:

```sh
k8s-request-bill examples/pods.json --usage examples/metrics.json
```

4. Voeg een prijs toe, om te zien wat het overschot kost:

```sh
k8s-request-bill examples/pods.json --usage examples/metrics.json --cpu-hour 0.031 --mem-gib-hour 0.004
```

5. Wijs het naar je eigen `pods.json` en `metrics.json`, met je eigen tarieven.

## Gebruik

```sh
# alleen requests
k8s-request-bill pods.json

# requests tegen gebruik, geprijsd per vCPU-uur en per GiB-uur
k8s-request-bill pods.json --usage metrics.json --cpu-hour 0.031 --mem-gib-hour 0.004

# één gecombineerd tarief: de uurprijs van een node, verdeeld naar zijn vorm
k8s-request-bill pods.json --usage metrics.json \
  --node-hour 0.192 --node-vcpu 4 --node-gib 16 --hours 730
```

## Voorbeeld

`examples/` bevat een kleine `pods.json` en `metrics.json`.
`examples/expected.txt` en `examples/expected.json` zijn de exacte uitvoer van

```sh
k8s-request-bill examples/pods.json --usage examples/metrics.json \
  --cpu-hour 0.031 --mem-gib-hour 0.004 --limits
```

```text
NAMESPACE    PODS  CPU_REQ  MEM_REQ  CPU_LIM  MEM_LIM  CPU_USE  MEM_USE  CPU_OVER  MEM_OVER  REQ_COST  OVER_COST
batch           1    1.000    2.000    0.000    0.000    0.050    0.098     0.950     1.902    0.0390     0.0371
kube-system     1    0.100    0.068    0.000    0.166    0.003    0.020     0.097     0.049    0.0034     0.0032
monitoring      1    2.000    4.000    0.000    0.000    0.000    0.000     0.000     0.000    0.0780     0.0000
shop            3    3.000    9.000    6.000   10.000    0.810    6.566     2.190     2.434    0.1290     0.0776
TOTAL           6    6.100   15.068    6.000   10.166    0.863    6.684     3.237     4.385    0.2494     0.1179
```

## Opties

### Tarieven

Prijzen zijn gewone decimalen in de munt die je gebruikt; `--currency` bepaalt
alleen het label naast de getallen. Zonder tariefvlag wordt geen kost getoond.

| Vlag | Betekenis |
| --- | --- |
| `--cpu-hour PRICE` | prijs van één vCPU voor één uur |
| `--mem-gib-hour PRICE` | prijs van één GiB geheugen voor één uur |
| `--node-hour PRICE` | één gecombineerd tarief: de uurprijs van een hele node. Vereist `--node-vcpu` en `--node-gib`. Niet te combineren met `--cpu-hour` / `--mem-gib-hour` |
| `--node-vcpu N` | vCPU's van die node (groter dan nul) |
| `--node-gib N` | GiB geheugen van die node (groter dan nul) |
| `--cpu-share F` | deel van de nodeprijs voor CPU, `0` tot `1`, standaard `0.5`; de rest gaat naar geheugen |
| `--hours H` | uren die de kost dekt, standaard `1` (`730` is ongeveer een maand) |
| `--currency CODE` | alleen een label, standaard `USD`; geen omrekening |

Met `--node-hour` is het CPU-tarief `node-hour x cpu-share / node-vcpu` en het
geheugentarief `node-hour x (1 - cpu-share) / node-gib`. Met maar één van
`--cpu-hour` en `--mem-gib-hour` wordt de andere resource op nul geprijsd.

`REQ_COST` is de kost van alles wat is aangevraagd; `OVER_COST` (met `--usage`)
is de kost van het overprovisioned deel.

### Overige opties

| Vlag | Betekenis |
| --- | --- |
| `PODS` | pods-JSON-bestand, `-` voor stdin |
| `-u, --usage FILE` | metrics-JSON-bestand, `-` voor stdin (niet beide) |
| `-n, --namespace NS` | alleen deze namespaces; herhaal of scheid met komma's |
| `--limits` | voeg de kolommen `CPU_LIM` en `MEM_LIM` toe |
| `--sort KEY` | `name` (standaard), `cpu-request`, `memory-request`, `cpu-over`, `memory-over`, `over-cost`; de laatste drie vragen `--usage`, `over-cost` vraagt ook een tarief |
| `--format table\|json` | uitvoerformaat, standaard `table` |

## Hoe het telt

- **Requests per pod** volgen de regel van de scheduler: het grootste van de
  piek van de init-containers en de som van de app-containers, waarbij
  herstartbare init-containers (sidecars) blijven draaien, plus
  `spec.overhead`. Limits gebruiken dezelfde regel; een container zonder limit
  telt niets bij `CPU_LIM` / `MEM_LIM`.
- Pods in fase `Succeeded` of `Failed` blijven buiten beschouwing (ze houden
  geen requests vast). Items in een lijst die geen pod zijn worden overgeslagen.
  Beide worden in een notitie geteld.
- **Gebruik** is de som van het containergebruik in het metricsbestand,
  gekoppeld aan pods op namespace en naam.
- **Overprovisioning** is de som over pods van `max(request - gebruik, 0)`. Het
  wordt per pod afgekapt, zodat een pod die boven zijn request uitkomt het
  overschot van een andere pod niet verbergt. Pods zonder metrics-regel tellen
  mee in de requests maar niet in `USE` of `OVER`; een notitie meldt hoeveel.
- Hoeveelheden: `m`, `u`, `n` voor CPU; `Ki` tot `Ei`, `k` tot `E`, `m` en
  exponent-notatie voor geheugen. Negatieve waarden worden geweigerd.
- Tabelgetallen zijn op drie decimalen afgerond (kosten op vier); JSON bevat
  meer precisie (vCPU exact, GiB en kosten op zes decimalen) als getallen.

## JSON

`--format json` print één document met `tool`, `version`, `hours`, `currency`,
`rates` (of `null`), `namespaces`, `total` en `notes`. Velden die gebruik of een
tarief nodig hebben zijn `null` als die invoer ontbrak.

## Afsluitcodes

`0` gelukt, `1` onleesbare of ongeldige invoer (de melding noemt bestand, pod en
container), `2` foute opdrachtregel, of standaarduitvoer die bij de start gesloten is
(`>&-`; een echte `/dev/null` is goed en geeft `0`).

## Beperkingen

Geen live clustertoegang, geen prijslijst, geen controllers (deployments,
jobs), geen VPA- of HPA-advies, geen bin-packing per node. Alleen de pods die je
aanlevert.

## Onderhoud

Klein hulpmiddel met één doel, onderhouden door één persoon. Bugs en patches
komen binnen via Issues; zie `CONTRIBUTING.md`. Beveiligingsmeldingen volgen
`SECURITY.md`. Wijzigingen staan in `CHANGELOG.md`. Een patch hoort een test te
hebben.

## Kwaliteit

169 tests; formattering en clippy met `-D warnings` zijn schoon.
Zie [QUALITY.nl.md](QUALITY.nl.md) voor de kaart van bewering naar test.

## Licentie

MIT OR Apache-2.0, naar keuze. Zie `LICENSE-MIT` en `LICENSE-APACHE`.

De licenties van de externe crates in het programma staan in [THIRD-PARTY-LICENSES.html](THIRD-PARTY-LICENSES.html).
