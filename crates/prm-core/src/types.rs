//! Reference types with type-specific fields, help text and interchange mappings.

use serde::Serialize;

#[derive(Debug, Clone, Serialize)]
pub struct RefType {
    pub id: &'static str,
    pub label: &'static str,
    pub group: &'static str,
    pub help: &'static str,
    /// Fields shown for this type, in order. Common fields (title, creators, date, url, doi,
    /// abstract, language, note) are always available.
    pub fields: &'static [&'static str],
    /// Creator roles offered for this type.
    pub roles: &'static [&'static str],
    pub ris: &'static str,
    pub bibtex: &'static str,
    pub csl: &'static str,
}

macro_rules! t {
    ($id:literal, $label:literal, $group:literal, $help:literal, [$($f:literal),*], [$($r:literal),*], $ris:literal, $bib:literal, $csl:literal) => {
        RefType { id: $id, label: $label, group: $group, help: $help, fields: &[$($f),*], roles: &[$($r),*], ris: $ris, bibtex: $bib, csl: $csl }
    };
}

pub static TYPES: &[RefType] = &[
    t!("article-journal", "Journal article", "Articles", "An article in a scholarly journal. Enter the article number separately from page ranges.", ["container_title", "volume", "issue", "page", "article_number", "issn", "pmid", "pmcid"], ["author", "translator"], "JOUR", "article", "article-journal"),
    t!("article-magazine", "Magazine article", "Articles", "An article in a magazine.", ["container_title", "volume", "issue", "page"], ["author"], "MGZN", "article", "article-magazine"),
    t!("article-newspaper", "Newspaper article", "Articles", "An article in a newspaper, print or online.", ["container_title", "section", "page", "publisher_place"], ["author"], "NEWS", "article", "article-newspaper"),
    t!("preprint", "Preprint", "Articles", "A manuscript posted to a preprint server before or instead of journal publication. Link it to the published version when one exists.", ["publisher", "number", "arxiv", "version"], ["author"], "UNPB", "misc", "article"),
    t!("review", "Review", "Articles", "A review of a book, film or other work.", ["container_title", "volume", "issue", "page", "reviewed_title"], ["author", "reviewed-author"], "JOUR", "article", "review"),
    t!("editorial", "Editorial", "Articles", "An editorial or commentary in a journal or newspaper.", ["container_title", "volume", "issue", "page"], ["author"], "JOUR", "article", "article-journal"),
    t!("book", "Book", "Books", "An authored book. Use Edited book when the book has editors instead of authors.", ["publisher", "publisher_place", "edition", "volume", "number_of_volumes", "isbn", "collection_title"], ["author", "translator", "editor"], "BOOK", "book", "book"),
    t!("edited-book", "Edited book", "Books", "A book with editors rather than authors.", ["publisher", "publisher_place", "edition", "volume", "isbn", "collection_title"], ["editor", "translator"], "EDBOOK", "book", "book"),
    t!("chapter", "Chapter in an edited book", "Books", "A chapter in an edited book. Enter the book's editors with the Editor role.", ["container_title", "publisher", "publisher_place", "edition", "volume", "page", "isbn"], ["author", "editor", "translator"], "CHAP", "incollection", "chapter"),
    t!("entry-encyclopedia", "Encyclopedia entry", "Books", "An entry in an encyclopedia.", ["container_title", "publisher", "edition", "volume", "page"], ["author", "editor"], "ENCYC", "incollection", "entry-encyclopedia"),
    t!("entry-dictionary", "Dictionary entry", "Books", "An entry in a dictionary.", ["container_title", "publisher", "edition"], ["author", "editor"], "DICT", "incollection", "entry-dictionary"),
    t!("classic", "Classic or religious work", "Books", "A classic or religious work cited by section rather than page; record the original date separately.", ["publisher", "edition", "original_publisher"], ["author", "translator", "editor"], "CLSWK", "book", "classic"),
    t!("report", "Report", "Grey literature", "A report from a government agency, organization or research group.", ["publisher", "publisher_place", "number", "genre", "collection_title"], ["author", "editor"], "RPRT", "techreport", "report"),
    t!("thesis", "Thesis or dissertation", "Grey literature", "A thesis or dissertation. Enter the degree in Genre and the institution in Publisher.", ["publisher", "genre", "number", "archive"], ["author"], "THES", "phdthesis", "thesis"),
    t!("paper-conference", "Conference paper", "Conferences", "A paper published in conference proceedings.", ["container_title", "event", "event_place", "publisher", "page", "volume"], ["author", "editor"], "CPAPER", "inproceedings", "paper-conference"),
    t!("presentation", "Conference presentation or poster", "Conferences", "An unpublished presentation, poster or symposium contribution.", ["event", "event_place", "genre", "event_date"], ["author", "chair"], "CONF", "misc", "speech"),
    t!("proceedings", "Conference proceedings", "Conferences", "A whole proceedings volume.", ["event", "publisher", "publisher_place", "volume"], ["editor"], "CONF", "proceedings", "book"),
    t!("webpage", "Web page", "Web", "A page on a website. Record an access date only when the content is designed to change.", ["container_title", "publisher"], ["author"], "ELEC", "online", "webpage"),
    t!("post-weblog", "Blog post", "Web", "A post on a blog.", ["container_title"], ["author"], "BLOG", "online", "post-weblog"),
    t!("post", "Social media post", "Web", "A post on a social media platform. Keep the original spelling and emoji.", ["container_title", "medium"], ["author"], "ELEC", "online", "post"),
    t!("dataset", "Dataset", "Data and software", "A published dataset. Enter the version and repository.", ["publisher", "version", "number", "medium"], ["author", "contributor"], "DATA", "misc", "dataset"),
    t!("software", "Software", "Data and software", "Software or a code repository. Enter the version.", ["publisher", "version", "medium"], ["author", "contributor"], "COMP", "software", "software"),
    t!("standard", "Standard", "Grey literature", "A technical standard from a standards body.", ["publisher", "number", "status"], ["author"], "STAND", "techreport", "standard"),
    t!("patent", "Patent", "Legal and government", "A patent. Enter the patent number in Number and the issuing office in Publisher.", ["number", "publisher", "jurisdiction"], ["inventor", "author"], "PAT", "patent", "patent"),
    t!("legal-case", "Court case", "Legal and government", "A court decision. Output may need manual review for legal citation styles.", ["container_title", "volume", "page", "authority", "jurisdiction"], [], "CASE", "misc", "legal_case"),
    t!("legislation", "Statute or legislation", "Legal and government", "A statute or act. Output may need manual review.", ["container_title", "section", "jurisdiction", "number"], [], "STAT", "misc", "legislation"),
    t!("bill", "Bill or resolution", "Legal and government", "An unenacted bill or resolution.", ["number", "authority", "jurisdiction", "container_title"], [], "BILL", "misc", "bill"),
    t!("regulation", "Regulation", "Legal and government", "A regulation or administrative rule.", ["container_title", "volume", "section", "jurisdiction"], [], "GOVDOC", "misc", "regulation"),
    t!("government-document", "Government document", "Legal and government", "A document issued by a government body.", ["publisher", "number", "publisher_place"], ["author"], "GOVDOC", "misc", "report"),
    t!("hearing", "Hearing", "Legal and government", "A legislative hearing.", ["authority", "number", "container_title"], [], "HEAR", "misc", "hearing"),
    t!("treaty", "Treaty", "Legal and government", "An international treaty or agreement.", ["container_title", "volume", "page"], [], "GOVDOC", "misc", "treaty"),
    t!("manuscript", "Unpublished manuscript", "Archival and personal", "An unpublished or informally published manuscript.", ["publisher", "genre", "archive"], ["author"], "UNPB", "unpublished", "manuscript"),
    t!("archival", "Archival material", "Archival and personal", "Material held in an archive. Online records are not required; enter collection and location.", ["archive", "archive_location", "collection_title", "medium"], ["author", "recipient"], "MANSCPT", "misc", "manuscript"),
    t!("letter", "Letter", "Archival and personal", "A letter or correspondence, published or archival.", ["archive", "archive_location", "medium"], ["author", "recipient"], "PCOMM", "misc", "letter"),
    t!("personal-communication", "Personal communication", "Archival and personal", "An email, conversation or other unrecoverable communication. Many styles cite these in text only.", ["medium"], ["author", "recipient"], "PCOMM", "misc", "personal_communication"),
    t!("interview", "Interview", "Archival and personal", "A published or archived interview.", ["container_title", "medium", "archive"], ["author", "interviewer"], "GEN", "misc", "interview"),
    t!("film", "Film or video", "Media", "A film, video or online video.", ["publisher", "medium", "container_title"], ["director", "producer", "author"], "VIDEO", "misc", "motion_picture"),
    t!("broadcast", "Television or radio episode", "Media", "An episode of a television or radio series.", ["container_title", "number", "publisher", "medium"], ["director", "producer", "author"], "VIDEO", "misc", "broadcast"),
    t!("podcast", "Podcast episode", "Media", "An episode of a podcast.", ["container_title", "number", "publisher", "medium"], ["host", "producer", "guest"], "SOUND", "misc", "broadcast"),
    t!("song", "Music recording", "Media", "A song or album recording.", ["container_title", "publisher", "medium"], ["author", "performer", "composer"], "MUSIC", "misc", "song"),
    t!("graphic", "Image or artwork", "Media", "A photograph, artwork, figure or other image.", ["publisher", "medium", "archive", "dimensions"], ["author"], "ART", "misc", "graphic"),
    t!("map", "Map", "Media", "A map or chart.", ["publisher", "publisher_place", "scale", "medium"], ["author"], "MAP", "misc", "map"),
    t!("performance", "Performance", "Media", "A live performance.", ["event", "event_place", "publisher"], ["author", "director", "performer"], "GEN", "misc", "performance"),
    t!("speech", "Speech or lecture", "Media", "A speech, lecture or sermon.", ["event", "event_place", "genre"], ["author"], "GEN", "misc", "speech"),
    t!("test-instrument", "Test or measure", "Data and software", "A psychological test, scale or inventory.", ["publisher", "version", "container_title"], ["author"], "GEN", "misc", "document"),
    t!("pamphlet", "Brochure or pamphlet", "Grey literature", "A brochure, pamphlet or fact sheet.", ["publisher", "publisher_place"], ["author"], "PAMP", "booklet", "pamphlet"),
    t!("document", "Other document", "Other", "Any other document. Output needs manual review.", ["publisher", "genre", "medium"], ["author", "editor"], "GEN", "misc", "document"),
];

pub fn get(id: &str) -> Option<&'static RefType> {
    TYPES.iter().find(|t| t.id == id)
}

pub fn label(id: &str) -> &'static str {
    get(id).map(|t| t.label).unwrap_or("Other document")
}

/// Fields every type has.
pub const COMMON_FIELDS: &[&str] = &["title", "short_title", "doi", "url", "abstract", "language", "note", "keywords"];

/// Human field labels and help.
pub fn field_label(f: &str) -> &'static str {
    match f {
        "title" => "Title",
        "short_title" => "Short title",
        "container_title" => "Published in",
        "volume" => "Volume",
        "issue" => "Issue",
        "page" => "Pages",
        "article_number" => "Article number",
        "doi" => "DOI",
        "url" => "URL",
        "publisher" => "Publisher",
        "publisher_place" => "Place",
        "edition" => "Edition",
        "isbn" => "ISBN",
        "issn" => "ISSN",
        "pmid" => "PubMed ID",
        "pmcid" => "PMC ID",
        "arxiv" => "arXiv ID",
        "abstract" => "Abstract",
        "language" => "Language",
        "note" => "Extra",
        "keywords" => "Keywords",
        "genre" => "Genre or degree",
        "medium" => "Medium or format",
        "number" => "Number",
        "version" => "Version",
        "collection_title" => "Series",
        "event" => "Event",
        "event_place" => "Event place",
        "event_date" => "Event date",
        "archive" => "Archive or repository",
        "archive_location" => "Location in archive",
        "section" => "Section",
        "jurisdiction" => "Jurisdiction",
        "authority" => "Court or authority",
        "number_of_volumes" => "Number of volumes",
        "reviewed_title" => "Reviewed work",
        "original_publisher" => "Original publisher",
        "status" => "Status",
        "dimensions" => "Dimensions",
        "scale" => "Scale",
        _ => "Field",
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn at_least_forty_types_with_unique_ids() {
        let ids: std::collections::BTreeSet<_> = super::TYPES.iter().map(|t| t.id).collect();
        assert!(super::TYPES.len() >= 40);
        assert_eq!(ids.len(), super::TYPES.len());
    }
}
