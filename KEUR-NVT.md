# KEUR-NVT

Stukken die de documentatiestandaard noemt en die in deze repository bewust
ontbreken, met de reden. Een regel per stuk: `stuk: reden`.

build.rs: er is geen buildscript nodig, want de tool genereert tijdens het bouwen niets en bindt geen systeembibliotheken.
man: er is geen man-pagina; de hulp staat in `--help` en in README.md.
docs: er is geen aparte documentatiemap; alles staat in README.md, QUALITY.md en CHANGELOG.md.
samples: voorbeeldinvoer staat in de map examples, een aparte samples-map is er niet.
scripts: er zijn geen hulpscripts; bouwen en testen gaat met cargo.
.github/workflows/release.yml: er is geen releaseworkflow, want de tool is nog niet gepubliceerd en er worden geen binaries verspreid.
