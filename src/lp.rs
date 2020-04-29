
use rust_stemmers::{Algorithm, Stemmer};
use ngrams::Ngram;
use regex::Regex;

pub fn get_first_chars(words: Vec<String>) -> Vec<String> {
  return words.iter().map(|word| {
    let first_char = String::from(word.chars().nth(0).unwrap().to_string());
    return String::from(format!("${}", first_char));
  }).collect();
}

pub fn pad_string(s: String) -> String {
  String::from(format!("${}$", s))
}

pub fn clean_words(s: String) -> Vec<String> {
  let en_stemmer = Stemmer::create(Algorithm::English);
  let regex = Regex::new(r"[^a-zA-Z0-9]").unwrap();
  let result = regex.replace_all(&s, " ").to_lowercase();
  return get_words(result)
    .iter()
    .filter(|x| x.len() >= 2)
    .map(|x| String::from(en_stemmer.stem(x)))
    .collect();
}

pub fn get_words(s: String) -> Vec<String> {
  return s.split(" ")
    .map(|x| String::from(x))
    .collect();
}

pub fn gramify(s: String) -> Vec<String> {
  if s.len() == 1 {
    return vec!(
      String::from(format!("${}", s))
    );
  }
  if s.len() == 2 {
    return vec!(
      String::from(format!("${}", String::from(s.chars().nth(0).unwrap().to_string()))),
      String::from(format!("{}$", String::from(s.chars().nth(1).unwrap().to_string())))
    );
  }
  if s.len() > 2 {
    let prepared_string: String = clean_words(s.clone()).join(" ");

    let mut tokens: Vec<String> = Vec::new();

    if prepared_string.len() > 0 {
      let grams: Vec<_> = prepared_string.chars().ngrams(3).collect();
      for gram in grams.into_iter() {
        let s: Vec<String> = gram.into_iter().map(|x| x.to_string()).collect();
        tokens.push(s.join(""));
      }
    }

    for c in get_first_chars(get_words(s)) {
      tokens.push(c.to_lowercase());
    }

    return tokens;
  }
  return vec!();
}