use std::collections::HashMap;
use serde_json::{Value};
// use rocket_contrib::json::{JsonValue};
use strsim::normalized_levenshtein;
// use serde_json::to_string;

use crate::lp::{gramify, clean_words};

/* fn parse_json(datastr: String) -> Value {
  return serde_json::from_str(&datastr).unwrap();
} */

pub struct Index {
  id_counter: u32,
  pub items: HashMap<u32, String>,
  pub token_scoring: HashMap<String, Vec<(u32, u8)>>,
  pub id_map: HashMap<String, u32>,
}

pub fn create() -> Index {
  Index {
    id_counter: 0,
    items: HashMap::new(),
    token_scoring: HashMap::new(),
    id_map: HashMap::new(),
  }
}

pub fn extract_fields(obj: &Value, fields: Vec<String>) -> String {
  let mut token_str = String::from("");
  
  for prop in fields.iter() {
    let value = &obj[prop];
    if value.is_string() {
      token_str.push_str(value.as_str().unwrap());
      token_str.push(' ');
    }
    else if value.is_array() {
      let vec: Vec<_> = value.as_array().unwrap().to_vec();
      for el in vec {
        if el.is_string() {
          token_str.push_str(el.as_str().unwrap());
          token_str.push(' ');
        }
      }
    }
    else if value.is_object() {
      let map = value.as_object().unwrap();
      for el in map.values() {
        if el.is_string() {
          token_str.push_str(el.as_str().unwrap());
          token_str.push(' ');
        }
      }
    }
  }

  return token_str;
}

pub fn remove(index: &mut Index, id: String) -> bool {
  let iid_maybe = index.id_map.get(&id.clone());

  if iid_maybe.is_none() {
    return false;
  }

  let iid = *iid_maybe.unwrap();

  index.items.remove(&iid);
  index.id_map.remove(&id);

  for value in index.token_scoring.values_mut() {
    value.retain(|x| x.0 != iid);
  }

  index.token_scoring.retain(|_, x| x.len() > 0);

  return true;
}

pub fn update(index: &mut Index, obj: Value, fields: Vec<String>) {
  let token_str = extract_fields(&obj, fields);
  let id = &obj["_id"].as_str().unwrap();
  let iid = *index.id_map.get(id.clone()).unwrap();

  for value in index.token_scoring.values_mut() {
    value.retain(|x| x.0 != iid);
  }

  index.token_scoring.retain(|_, x| x.len() > 0);

  index.items.insert(iid as u32, obj.to_string());
  index_item(index, iid, token_str.trim().to_string());
}

pub fn add_object(index: &mut Index, obj: Value, fields: Vec<String>) {
  let token_str = extract_fields(&obj, fields);
  let id = &obj["_id"].as_str().unwrap();

  add(
    index,
    id.to_string(),
    obj.to_string(),
    token_str.trim().to_string()
  );
}

fn add(index: &mut Index, id: String, obj: String, to_tokenize: String) {
  let iid = index.id_counter;
  index.id_map.insert(id, iid);
  index.id_counter += 1;
  
  index.items.insert(iid as u32, obj);
  index_item(index, iid, to_tokenize);
}

fn index_item(index: &mut Index, iid: u32, to_tokenize: String) {
  for gram in gramify(to_tokenize.to_string()).iter() {
    if !index.token_scoring.contains_key(&gram.clone()) {
      index.token_scoring.insert(gram.to_string(), vec![(iid as u32, 1)]);
    } else {
      index.token_scoring.get_mut(gram).unwrap().push((iid as u32, 1));
    }
  }

  for word in clean_words(to_tokenize.to_string()) {
    if !index.token_scoring.contains_key(&word.clone()) {
      index.token_scoring.insert(word.to_string(), vec![(iid as u32, 10)]);
    } else {
      index.token_scoring.get_mut(&word).unwrap().push((iid as u32, 10));
    }
  }
}

fn get_key_score_list(index: &Index, query: String) -> Vec<(u32, f32)> {
  let mut scores: HashMap<u32, f32> = HashMap::new();
  let mut query_tokens = gramify(query.clone());
  
  for word in clean_words(query.clone()) {
    query_tokens.push(word.clone());
  }

  for token in query_tokens.iter() {
    if index.token_scoring.contains_key(&token.clone()) {
      let ids = index.token_scoring.get(token).unwrap();
      
      for id in ids.iter() {
        *scores.entry(id.0).or_insert(0.0) += id.1 as f32;
      }
    }
  }
  
  let mut key_score_list: Vec<(u32, f32)> = Vec::new();
  for (id, score) in scores {
    let name = index.items.get(&id).unwrap();
    let dist_score = normalized_levenshtein(&query, &name) as f32;
    key_score_list.push((id, score + dist_score));
  }

  key_score_list.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap());

  return key_score_list;
}

pub fn search(index: &Index, original_query: String) -> Vec<String> {
  let query = original_query.trim();

  if query.len() == 0 {
    let mut vec: Vec<String> = Vec::new();
    for item in index.items.values() {
      vec.push(item.clone());
    }
    return vec;
  }
  
  let mut key_score_list = get_key_score_list(&index, query.to_string());

  if key_score_list.len() == 0 {
    return Vec::new();
  }

  let highest = key_score_list[0].1;
  key_score_list.retain(|x| x.1 > highest / 2.0);
  
  let mut real_items : Vec<String> = vec![];
  for tuple in key_score_list.iter_mut() {
    let item = index.items.get(&tuple.0).unwrap().clone();
    real_items.push(item);
  }
  return real_items;
}
