use kiss::Language;

pub struct DiscoverArgs<'a> {
    pub universe: &'a str,
    pub paths: &'a [String],
    pub lang_filter: Option<Language>,
    pub ignore: &'a [String],
}
