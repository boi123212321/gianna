#![feature(proc_macro_hygiene, decl_macro)]

#[macro_use]
extern crate rocket;
#[macro_use]
extern crate rocket_contrib;
#[macro_use]
extern crate serde_derive;
extern crate strsim;
extern crate md5;
extern crate rand;

use lazy_static::lazy_static;
use rocket::config::{Config, Environment, Limits};
use rocket_contrib::json::{Json, JsonValue};
use serde_json::{Value};
use std::collections::HashMap;
use std::sync::Mutex;
use rocket::http::{Status, ContentType};
use rocket::response;
use rocket::response::{Responder, Response};
use rocket::request::Request;
use std::env;
use rand::seq::SliceRandom;
use rand::prelude::*;

mod lp;
mod index;

lazy_static! {
  static ref INDEXES: Mutex<HashMap<String, index::Index>> = Mutex::new(HashMap::new());
}

fn parse_json(datastr: String) -> Value {
  return serde_json::from_str(&datastr).unwrap();
}

#[derive(Debug)]
struct ApiResponse {
  json: JsonValue,
  status: Status,
}

impl<'r> Responder<'r> for ApiResponse {
  fn respond_to(self, req: &Request) -> response::Result<'r> {
    Response::build_from(self.json.respond_to(&req).unwrap())
      .status(self.status)
      .header(ContentType::JSON)
      .ok()
  }
}

#[derive(Clone, Serialize, Deserialize)]
struct BulkImport {
  items: Vec<Value>,
  fields: Vec<String>
}

#[derive(Clone, Serialize, Deserialize)]
struct FilterCondition {
  property: String,
  r#type: String,
  operation: String,
  value: Value,
}

#[derive(Clone, Serialize, Deserialize)]
struct FilterTree {
  r#type: Option<String>, // AND or OR
  children: Option<Vec<FilterTree>>,
  condition: Option<FilterCondition>,
}

#[derive(Clone, Serialize, Deserialize)]
struct SearchOptions {
  filter: Option<FilterTree>,

  sort_by: Option<String>,
  sort_asc: Option<bool>,
  sort_type: Option<String>,
}

fn dot_notation(obj: &Value, path: String) -> Value {
  let keys: Vec<String> = path.split(".").map(String::from).collect();

  let mut curr = obj;
  for key in keys {
    curr = &curr[key];
  }

  return curr.clone();
}

fn check_tree_node(tree: &FilterTree, obj: &Value) -> bool {
  if tree.children.is_some() {
    let filter_type = tree.r#type.as_ref().unwrap();
    let children = tree.children.as_ref().unwrap();
    if filter_type.cmp(&String::from("AND")) == std::cmp::Ordering::Equal {
      for child in children {
        let result = check_tree_node(&child, obj);
        if !result {
          return false;
        }
      }
      return true;
    }
    else if filter_type.cmp(&String::from("OR")) == std::cmp::Ordering::Equal  {
      for child in children {
        let result = check_tree_node(&child, obj);
        if result {
          return true;
        }
      }
      return false;
    }
    else if filter_type.cmp(&String::from("NOT")) == std::cmp::Ordering::Equal  {
      let result = check_tree_node(&children[0], obj);
      return !result;
    }
  }
  else {
    let condition = tree.condition.as_ref().unwrap();
    let r#type = &condition.r#type;
    let value = dot_notation(obj, condition.property.clone());

    if r#type.cmp(&String::from("string")) == std::cmp::Ordering::Equal {
      let string = value.as_str().unwrap_or("");

      if condition.operation.cmp(&String::from("=")) == std::cmp::Ordering::Equal {
        let other_string = &condition.value.as_str().unwrap_or("");
        return string.cmp(other_string) == std::cmp::Ordering::Equal;
      }
      else if condition.operation.cmp(&String::from("?")) == std::cmp::Ordering::Equal {
        let other_string = &condition.value.as_str().unwrap_or("");
        return string.contains(other_string);
      }
    }

    if r#type.cmp(&String::from("number")) == std::cmp::Ordering::Equal {
      let number = if value.is_f64() { 
        value.as_f64().unwrap_or(0.0) 
      } else { 
        value.as_i64().unwrap_or(0) as f64
      };
      
      if condition.operation.cmp(&String::from("=")) == std::cmp::Ordering::Equal {
        let other_number = &condition.value.as_f64().unwrap_or(0.0);
        return (number - other_number).abs() < 0.000001;
      }
      else if condition.operation.cmp(&String::from(">")) == std::cmp::Ordering::Equal {
        let other_number = &condition.value.as_f64().unwrap_or(0.0);
        return number > *other_number;
      }
      else if condition.operation.cmp(&String::from("<")) == std::cmp::Ordering::Equal {
        let other_number = &condition.value.as_f64().unwrap_or(0.0);
        return number < *other_number;
      }
    }

    if r#type.cmp(&String::from("array")) == std::cmp::Ordering::Equal {
      let array: Vec<Value> = value.as_array().unwrap_or(&Vec::new()).to_vec();

      if condition.operation.cmp(&String::from("?")) == std::cmp::Ordering::Equal {
        let other_value = &condition.value;
        return array.contains(other_value);
      }
      else if condition.operation.cmp(&String::from("length")) == std::cmp::Ordering::Equal {
        let length = condition.value.as_u64().unwrap_or(0) as usize;
        return array.len() == length;
      }
    }

    if r#type.cmp(&String::from("boolean")) == std::cmp::Ordering::Equal {
      let boolean = value.as_bool().unwrap_or(false);

      if condition.operation.cmp(&String::from("=")) == std::cmp::Ordering::Equal {
        let other_boolean = condition.value.as_bool().unwrap_or(false);
        return boolean == other_boolean;
      }
    }

    if r#type.cmp(&String::from("null")) == std::cmp::Ordering::Equal {
      if condition.operation.cmp(&String::from("=")) == std::cmp::Ordering::Equal {
        return value.is_null();
      }
    }
  }
  return false;
}

fn get_items(index: &index::Index, query: Option<String>) -> Vec<Value> {
  let items: Vec<Value>;

  if query.clone().is_some() {
    println!("Searching '{}'", query.clone().unwrap());
    let new_items = index::search(&index, String::from(query.clone().unwrap()));
    let mut vec: Vec<Value> = vec![];
    for item in new_items {
      vec.push(parse_json(item));
    }
    items = vec;
  }
  else {
    let mut vec: Vec<Value> = vec![];
    for id in index.items.values() {
      vec.push(parse_json(id.to_string()));
    }
    items = vec;
  }

  return items;
}

#[post("/<index_name>/search?<q>&<skip>&<take>", data="<input>")]
fn search_items(index_name: String, input: Json<SearchOptions>, q: Option<String>, skip: Option<u32>, take: Option<u32>) -> ApiResponse {
  let indexes = INDEXES.lock().unwrap();

  if indexes.contains_key(&index_name) {
    let index = indexes.get(&index_name).unwrap();
    let data = input.into_inner();

    // Get items
    let mut items = get_items(&index, q.clone());

    // Filter items
    if data.filter.is_some() {
      let filter_tree = data.filter.unwrap();
      items.retain(|x| check_tree_node(&filter_tree, &x));
    }
    let num_items = items.len();

    
    // Sort items
    if data.sort_by.is_some() {
      // Maybe shuffle
      let sort_prop = data.sort_by.unwrap();

      if sort_prop.cmp(&String::from("$shuffle")) == std::cmp::Ordering::Equal {
        let seed = data.sort_type.unwrap_or(String::from("default"));
        let hash = format!("{:x}", md5::compute(seed));
        let mut sum = 0 as u64;
        for c in hash.chars() {
          sum += c.to_digit(10).unwrap_or(0) as u64;
        }
        let mut rng = StdRng::seed_from_u64(sum);
        items.shuffle(&mut rng);
      }
      else {
        let sort_asc = data.sort_asc.unwrap_or(false);
        let sort_type = data.sort_type.unwrap_or(String::from("number"));
        
        items.sort_by(|a, b| {
          if sort_type.cmp(&String::from("number")) == std::cmp::Ordering::Equal {
            let a_maybe = dot_notation(a, sort_prop.clone());
            let b_maybe = dot_notation(b, sort_prop.clone());
            let a_val = a_maybe.as_f64().unwrap_or(0.0);
            let b_val = b_maybe.as_f64().unwrap_or(0.0);
            if sort_asc == true {
              return b_val.partial_cmp(&a_val).unwrap()
            }
            return a_val.partial_cmp(&b_val).unwrap()
          }
          else if sort_type.cmp(&String::from("string")) == std::cmp::Ordering::Equal {
            let a_maybe = dot_notation(a, sort_prop.clone());
            let b_maybe = dot_notation(b, sort_prop.clone());
            let a_val = a_maybe.as_str().unwrap_or("");
            let b_val = b_maybe.as_str().unwrap_or("");
            if sort_asc == true {
              return b_val.partial_cmp(&a_val).unwrap()
            }
            return a_val.partial_cmp(&b_val).unwrap()
          }
          return std::cmp::Ordering::Equal;
        });
      }
    }

    let _skip = skip.unwrap_or(0) as usize;
    let _take = take.unwrap_or(2000000) as usize;

    // Paginate items
    let page: Vec<_> = items
      .iter_mut()
      .rev()
      .skip(_skip)
      .take(_take)
      .collect();

    let ids: Vec<_> = page.iter().map(|x| {
      let obj = parse_json(x.to_string());
      let raw = obj["_id"].as_str().unwrap();
      return String::from(raw);
    }).collect();

    let num_pages = if ids.len() < _take { 
      1 
    } else {
      ((num_items / _take) as f32).ceil() as u32 
    };

    return ApiResponse {
      json: json!({
        "status": 200,
        "message": "Search successful",
        "query": q,
        "items": ids,
        "max_items": num_items,
        "num_items": ids.len(),
        "num_pages": num_pages,
      }),
      status: Status::Ok
    }
  }
  else {
    return ApiResponse {
      json: json!({
        "status": 404,
        "message": "Index not found"
      }),
      status: Status::NotFound
    }
  }
}

#[derive(Clone, Serialize, Deserialize)]
struct BulkDelete {
  items: Vec<String>,
}

#[delete("/<index_name>", data="<input>")]
fn delete_items(index_name: String, input: Json<BulkDelete>) -> ApiResponse {
  let mut indexes = INDEXES.lock().unwrap();
  
  if indexes.contains_key(&index_name) {
    let data = input.into_inner();
    let mut index = indexes.get_mut(&index_name).unwrap();
    
    for id in data.items {
      index::remove(&mut index, id);
    }

    return ApiResponse {
      json: json!({
        "status": 200,
        "message": "Items deleted"
      }),
      status: Status::Ok
    }
  }
  else {
    return ApiResponse {
      json: json!({
        "status": 404,
        "message": "Index not found"
      }),
      status: Status::NotFound
    }
  }
}
 

#[patch("/<index_name>", data="<input>")]
fn update_item(index_name: String, input: Json<BulkImport>) -> ApiResponse {
  let mut indexes = INDEXES.lock().unwrap();
  
  if indexes.contains_key(&index_name) {
    let data = input.into_inner();
    let mut index = indexes.get_mut(&index_name).unwrap();
    for item in data.items {
      index::update(
        &mut index, 
        item,
        data.fields.clone(),
      );
    }

    return ApiResponse {
      json: json!({
        "status": 200,
        "message": "Items updated"
      }),
      status: Status::Ok
    }
  }
  else {
    return ApiResponse {
      json: json!({
        "status": 404,
        "message": "Index not found"
      }),
      status: Status::NotFound
    }
  }
}

#[post("/<index_name>", data="<input>")]
fn post_items(index_name: String, input: Json<BulkImport>) -> ApiResponse {
  let mut indexes = INDEXES.lock().unwrap();
  let data = input.into_inner();

  if indexes.contains_key(&index_name) {
    let mut index = indexes.get_mut(&index_name).unwrap();
    for item in data.items {
      index::add_object(
        &mut index, 
        item,
        data.fields.clone(),
      );
    }

    return ApiResponse {
      json: json!({
        "status": 200,
        "message": "Items added"
      }),
      status: Status::Ok
    }
  }
  else {
    return ApiResponse {
      json: json!({
        "status": 404,
        "message": "Index not found"
      }),
      status: Status::NotFound
    }
  }
}

#[put("/<index_name>")]
fn create_index(index_name: String) -> ApiResponse {
  let mut indexes = INDEXES.lock().unwrap();

  if indexes.contains_key(&index_name) {
    return ApiResponse {
      json: json!({
        "status": 409,
        "message": "Index already exists",
        "error": true
      }),
      status: Status::Conflict
    }
  }
  else {
    let index = index::create();
    indexes.insert(index_name, index);
    return ApiResponse {
      json: json!({
        "status": 200,
        "message": "Index created"
      }),
      status: Status::Ok
    }
  }
}

#[delete("/<index_name>/delete", rank = 0)]
fn delete_index(index_name: String) -> Status {
  let mut indexes = INDEXES.lock().unwrap();

  if indexes.contains_key(&index_name) {
    indexes.remove(&index_name);
    return Status::Ok;
  }
  return Status::NotFound;
}

#[delete("/<index_name>/clear", rank = 0)]
fn clear_index(index_name: String) -> Status {
  let mut indexes = INDEXES.lock().unwrap();

  if indexes.contains_key(&index_name) {
    let index = indexes.get_mut(&index_name).unwrap();
    index::clear(index);
    return Status::Ok;
  }
  return Status::NotFound;
}

#[delete("/")]
fn clear_all() -> Status {
  let mut indexes = INDEXES.lock().unwrap();
  indexes.clear();
  indexes.shrink_to_fit();
  return Status::Ok;
}

#[get("/<index_name>")]
fn get_index(index_name: String) -> ApiResponse {
  let indexes = INDEXES.lock().unwrap();

  if indexes.contains_key(&index_name) {
    let index = indexes.get(&index_name).unwrap();
    return ApiResponse {
      json: json!({
        "status": 200,
        "items_count": index.items.len(),
        "tokens_count": index.token_scoring.len(),
      }),
      status: Status::Ok
    }
  }
  else {
    return ApiResponse {
      json: json!({
        "status": 404,
        "message": "Index not found",
        "error": true
      }),
      status: Status::NotFound
    }
  }
}

#[get("/")]
fn hello() -> Json<JsonValue> {
  Json(json!({
    "version": "0.0.2"
  }))
}

fn main() {
  let limits = Limits::new()
    .limit("forms", 5000000 * 1024 * 1024)
    .limit("json", 5000000 * 1024 * 1024);

  let mut config = Config::build(Environment::Production)
    .limits(limits)
    .unwrap();

  let args: Vec<String> = env::args().collect();

  config.port = 8001;

  for (i, arg) in args.iter().enumerate() {
    if arg.cmp(&String::from("--port")) == std::cmp::Ordering::Equal {
      let port_num = args[i + 1].parse();
      if !port_num.is_err() {
        config.port = port_num.unwrap();
      }
    }
  }

  let app = rocket::custom(config);

  app
    .mount("/", routes![hello])
    .mount("/index", routes![delete_index, clear_index, clear_all, delete_items, create_index, get_index, post_items, update_item, search_items])
    .launch();
}
