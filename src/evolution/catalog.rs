//! Stabilny katalog treści pierwszego wycinka kampanii.
//!
//! Daty i relacje są tu opisane jako obserwacje, hipotezy albo planowane
//! przyszłe rozdziały. Odblokowanie karty jest mechaniką postępu gry i nie
//! oznacza bezpośredniego pokrewieństwa biologicznego.

#[derive(Clone, Copy, Debug)]
pub struct Source {
    pub title: &'static str,
    pub compact: &'static str,
    pub url: &'static str,
}

#[derive(Clone, Copy, Debug)]
pub struct Discovery {
    pub id: &'static str,
    pub distance_km: f64,
    pub title: &'static str,
    pub summary: &'static str,
    pub uncertainty: &'static str,
    pub sources: &'static [Source],
}

const SOURCES_EARLY_LIFE: &[Source] = &[
    Source {
        title: "Dresser Formation — przekonujące dowody",
        compact: "Nature Communications · ncomms15263",
        url: "https://www.nature.com/articles/ncomms15263",
    },
    Source {
        title: "Starsze interpretacje — ostrożność",
        compact: "Nature · s41586-018-0610-4",
        url: "https://www.nature.com/articles/s41586-018-0610-4",
    },
];

const SOURCES_CELL: &[Source] = &[Source {
    title: "Rekonstrukcja i ograniczenia zapisu kopalnego",
    compact: "Nature Communications · ncomms15263",
    url: "https://www.nature.com/articles/ncomms15263",
}];

const SOURCES_COMMUNITIES: &[Source] = &[Source {
    title: "Formacja Dresser — wczesne społeczności",
    compact: "Nature Communications · ncomms15263",
    url: "https://www.nature.com/articles/ncomms15263",
}];

const SOURCES_HYPOTHESES: &[Source] = &[
    Source {
        title: "Putatywne starsze ślady — wynik niepewny",
        compact: "Nature · nature21377",
        url: "https://www.nature.com/articles/nature21377",
    },
    Source {
        title: "Ponowna ocena starszych struktur — ostrożność",
        compact: "Nature · s41586-018-0610-4",
        url: "https://www.nature.com/articles/s41586-018-0610-4",
    },
    Source {
        title: "Model molekularny — nieobserwowany początek",
        compact: "Nature Ecology & Evolution · s41559-018-0644-x",
        url: "https://www.nature.com/articles/s41559-018-0644-x",
    },
];

pub const DISCOVERIES: &[Discovery] = &[
    Discovery {
        id: "early-life-evidence",
        distance_km: 0.0,
        title: "Ślady wczesnego życia",
        summary: "Około 3,5 mld lat temu życie było już obecne. Przekonujące dowody z formacji Dresser mają około 3,48 mld lat; nie mówią jednak, kiedy i gdzie życie powstało.",
        uncertainty: "To karta o obserwacjach i niepewności, nie dokładna data narodzin życia.",
        sources: SOURCES_EARLY_LIFE,
    },
    Discovery {
        id: "anonymous-single-cell",
        distance_km: 0.25,
        title: "Anonimowa pojedyncza komórka",
        summary: "Oglądasz uproszczoną rekonstrukcję jednej wczesnej komórki. Nie znamy skamieniałości, która ustalałaby jej dokładny kształt, kolor albo sposób ruchu.",
        uncertainty: "Kolory i sylwetka są ilustracją artystyczną; organizm pozostaje anonimowy.",
        sources: SOURCES_CELL,
    },
    Discovery {
        id: "microbial-communities",
        distance_km: 0.75,
        title: "Społeczności mikroorganizmów",
        summary: "Mikroorganizmy i ich środowiska mogły tworzyć różne społeczności na długo przed pojawieniem się zwierząt. Tło pokazuje jedną możliwą scenę, nie znanego skamieniałego przodka.",
        uncertainty: "Odblokowanie tej karty nie tworzy liniowego łańcucha przodków.",
        sources: SOURCES_COMMUNITIES,
    },
    Discovery {
        id: "multiple-origin-hypotheses",
        distance_km: 1.5,
        title: "Wiele hipotez środowiska",
        summary: "Wczesne życie mogło wiązać się z różnymi środowiskami wodnymi. Starsze ślady i hipotezy są dyskutowane; ta scena przedstawia tylko jeden wybrany, możliwy habitat.",
        uncertainty: "Nie jest to dowód na narodziny życia 4,0 mld lat temu ani rozstrzygnięcie jednej hipotezy.",
        sources: SOURCES_HYPOTHESES,
    },
];

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum UpgradeKind {
    Accuracy,
    Range,
}

#[derive(Clone, Copy, Debug)]
pub struct UpgradeSpec {
    pub title: &'static str,
    pub description: &'static str,
    pub costs: [f64; 2],
    pub max_level: u8,
}

pub const UPGRADES: &[UpgradeSpec] = &[
    UpgradeSpec {
        title: "Dokładność obserwacji",
        description: "Mechanika gry: zwiększa tempo zdobywania wiedzy o 25% na poziom.",
        costs: [25.0, 75.0],
        max_level: 2,
    },
    UpgradeSpec {
        title: "Zasięg obserwacji",
        description: "Mechanika gry: odsłania skromnie więcej dalszych form tła.",
        costs: [40.0, 100.0],
        max_level: 2,
    },
];

#[derive(Clone, Copy, Debug)]
pub struct RoadmapCard {
    pub title: &'static str,
    pub interval: &'static str,
    pub note: &'static str,
}

pub const ROADMAP: &[RoadmapCard] = &[
    RoadmapCard {
        title: "Mikroorganizmy i zmiany środowiska",
        interval: "następny etap",
        note: "Planowane: różne metabolizmy, maty i środowiska; wymagają osobnych źródeł.",
    },
    RoadmapCard {
        title: "Wielkie utlenienie",
        interval: "około 2,4 mld lat temu",
        note: "Planowane. Nie oznacza jednoczesnego natlenienia całego oceanu.",
    },
    RoadmapCard {
        title: "Eukarionty",
        interval: "najstarsze opisane skamieniałości około 1,75–1,4 mld lat",
        note: "Planowane. Daty skamieniałości nie są dokładną datą początku całej grupy.",
    },
    RoadmapCard {
        title: "Biota ediakarska",
        interval: "około 600–541 mln lat temu",
        note: "Planowane. Pokrewieństwo wielu form pozostaje niepewne.",
    },
    RoadmapCard {
        title: "Wczesne strunowce",
        interval: "około 520 mln lat temu",
        note: "Planowane. Przykładowa forma nie jest automatycznie przodkiem gracza.",
    },
    RoadmapCard {
        title: "Przejście czworonogów ku lądowi",
        interval: "późny dewon; starsze tropy około 390 mln lat",
        note: "Planowane. Nie jest to chwila pierwszego pojawienia się życia na lądzie.",
    },
    RoadmapCard {
        title: "Synapsydy",
        interval: "karbon i perm",
        note: "Planowane. To linia obejmująca ssaki, a nie dinozaury.",
    },
    RoadmapCard {
        title: "Wczesne formy ssacze",
        interval: "późny trias i jura",
        note: "Planowane. Mammaliaformes to nie to samo co grupa koronna ssaków.",
    },
    RoadmapCard {
        title: "Ssaki, naczelne i homininy",
        interval: "datowanie do opracowania dla kart",
        note: "Planowane. Gałęzie współistniały; nie tworzymy jednej drabiny gatunków.",
    },
    RoadmapCard {
        title: "Homo sapiens i sąsiednie linie Homo",
        interval: "nasz gatunek około 300 tys. lat temu",
        note: "Planowane. Nie ma tu fałszywego ulepszenia neandertalczyk → sapiens.",
    },
    RoadmapCard {
        title: "Życie ludzi i produkcja żywności",
        interval: "różne regiony; ostatnie około 12 tys. lat",
        note: "Planowane. To historia kultury tego samego gatunku, nie nowy gatunek.",
    },
    RoadmapCard {
        title: "Egipt oraz wybrane średniowieczne regiony",
        interval: "osobne historie regionalne",
        note: "Planowane. Zjednoczenie Egiptu około 3100 p.n.e. nie jest końcem całej historii.",
    },
];

pub const FIRST_CHAPTER_ID: &str = "archean-micro-world";
pub const FIRST_HABITAT_ID: &str = "watery-microenvironment";
pub const FIRST_ORGANISM_ID: &str = "anonymous-early-single-cell";

pub fn discovery(id: &str) -> Option<&'static Discovery> {
    DISCOVERIES.iter().find(|card| card.id == id)
}

pub fn is_known_discovery(id: &str) -> bool {
    discovery(id).is_some()
}

pub fn upgrade(kind: UpgradeKind) -> &'static UpgradeSpec {
    match kind {
        UpgradeKind::Accuracy => &UPGRADES[0],
        UpgradeKind::Range => &UPGRADES[1],
    }
}
