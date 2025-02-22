#![allow(non_snake_case)]

mod attr;
mod metadata;
mod error;
mod ext;
mod syndecode;

use std::collections::HashMap;
use std::fs;
use std::path::Path;
use syn::{DeriveInput, Item};
use ignore::Walk;

use syndecode::{Attributes};
pub use metadata::*;
pub use attr::*;
pub use error::*;
pub use ext::*;

#[derive(Default, Debug)]
pub struct LoadOptions {
    pub verbose: bool,
}

/// This is an intermediate representation of the schema.
///
pub struct OrmliteSchema {
    pub tables: Vec<ModelMetadata>,
    // map of rust structs (e.g. enums) to database encodings.
    // note that these are not bona fide postgres types.
    pub type_reprs: HashMap<String, String>,
}

type WithAttr<T> = (T, Attributes);

struct Intermediate {
    model_structs: Vec<WithAttr<syn::ItemStruct>>,
    type_structs: Vec<WithAttr<syn::ItemStruct>>,
    type_enums: Vec<WithAttr<syn::ItemEnum>>,
}

impl Intermediate {
    fn into_models_and_types(self) -> (impl Iterator<Item=WithAttr<syn::ItemStruct>>, impl Iterator<Item=WithAttr<String>>) {
        let models = self.model_structs.into_iter();
        let types = self.type_structs.into_iter().map(|(s, a)| (s.ident.to_string(), a))
            .chain(self.type_enums.into_iter().map(|(e, a)| (e.ident.to_string(), a)));
        (models, types)
    }

    fn from_with_opts(value: syn::File) -> Self {
        let mut model_structs = Vec::new();
        let mut type_structs = Vec::new();
        let mut type_enums = Vec::new();
        for item in value.items {
            match item {
                Item::Struct(s) => {
                    let mut attrs = Attributes::from(s.attrs.as_slice());
                    if attrs.has_derive("Model") {
                        attrs.retain("ormlite");
                        tracing::debug!(model=s.ident.to_string(), "Found");
                        model_structs.push((s, attrs));
                    } else if attrs.has_derive("Type") {
                        tracing::debug!(r#type=s.ident.to_string(), "Found");
                        type_structs.push((s, attrs));
                    } else if attrs.has_derive("ManualType") {
                        tracing::debug!(r#type=s.ident.to_string(), "Found");
                        type_structs.push((s, attrs));
                    }
                }
                Item::Enum(e) => {
                    let attrs = Attributes::from(e.attrs.as_slice());
                    if attrs.has_derive("Type") || attrs.has_derive("ManualType") {
                        type_enums.push((e, attrs));
                    }
                }
                _ => {}
            }
        }
        Self {
            model_structs,
            type_structs,
            type_enums,
        }
    }
}

pub fn schema_from_filepaths(paths: &[&Path]) -> anyhow::Result<OrmliteSchema> {
    let walk = paths.iter().flat_map(Walk::new);

    let walk = walk.filter_map(|e| e.ok())
        .filter(|e| e.path().extension().map(|e| e == "rs")
            .unwrap_or(false))
        .map(|e| e.into_path())
        .chain(paths.iter()
            .filter(|p| p.ends_with(".rs"))
            .map(|p| p.to_path_buf())
        );

    let mut tables = vec![];
    let mut type_aliases = HashMap::new();
    for entry in walk {
        let contents = fs::read_to_string(&entry)?;
        if !contents.contains("Model") {
            continue;
        }
        tracing::debug!(file=entry.display().to_string(), "Checking for derive attrs: Model, Type, ManualType");
        let ast = syn::parse_file(&contents)?;
        let intermediate = Intermediate::from_with_opts(ast);
        let (models, types) = intermediate.into_models_and_types();

        for (item, _attrs) in models {
            let derive: DeriveInput = item.into();
            let table = ModelMetadata::from_derive(&derive)?;
            tables.push(table);
        }

        for (name, attrs) in types {
            let mut ty = "String".to_string();
            for attr in attrs.iter_args() {
                if attr.name != "repr" {
                    continue;
                }
                ty = attr.args.first().expect("repr attribute must have at least one argument").clone();
            }
            type_aliases.insert(name, ty);
        }
    }
    Ok(OrmliteSchema {
        tables,
        type_reprs: type_aliases,
    })
}
