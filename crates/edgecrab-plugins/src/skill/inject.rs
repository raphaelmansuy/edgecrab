pub fn translate_hermes_paths(content: &str) -> String {
    content.replace("~/.hermes/", "~/.edgecrab/")
}

pub fn build_prompt_fragment(name: &str, body: &str) -> String {
    format!("## {name}\n\n{}", translate_hermes_paths(body).trim())
}
