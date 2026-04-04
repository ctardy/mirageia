/// Dictionnaires embarqués dans le binaire pour la génération de pseudonymes.
pub struct Dictionaries {
    pub firstnames: Vec<String>,
    pub lastnames: Vec<String>,
}

impl Dictionaries {
    pub fn load() -> Self {
        let firstnames: Vec<String> =
            serde_json::from_str(include_str!("../../dictionaries/firstnames.json"))
                .expect("firstnames.json invalide");
        let lastnames: Vec<String> =
            serde_json::from_str(include_str!("../../dictionaries/lastnames.json"))
                .expect("lastnames.json invalide");

        Self {
            firstnames,
            lastnames,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_load_dictionaries() {
        let dicts = Dictionaries::load();
        assert!(dicts.firstnames.len() >= 40);
        assert!(dicts.lastnames.len() >= 40);
    }

    #[test]
    fn test_no_duplicates() {
        let dicts = Dictionaries::load();
        let mut names = dicts.firstnames.clone();
        names.sort();
        names.dedup();
        assert_eq!(names.len(), dicts.firstnames.len(), "Doublons dans firstnames");

        let mut lasts = dicts.lastnames.clone();
        lasts.sort();
        lasts.dedup();
        assert_eq!(lasts.len(), dicts.lastnames.len(), "Doublons dans lastnames");
    }
}
