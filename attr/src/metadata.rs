use derive_builder::Builder;
use syn::{DeriveInput, Field, PathArguments};
use crate::{ColumnAttributes, ModelAttributes, SyndecodeError};
use crate::DeriveInputExt;
use convert_case::{Case, Casing};
use proc_macro2::TokenStream;
use quote::{TokenStreamExt};

#[derive(Clone, Debug, Hash, PartialEq, Eq)]
pub struct Ident(pub String);

impl Ident {
    pub fn new(ident: &str) -> Self {
        Ident(ident.to_string())
    }
    pub fn as_ref(&self) -> &String {
        &self.0
    }
}

impl From<&proc_macro2::Ident> for Ident {
    fn from(ident: &proc_macro2::Ident) -> Self {
        Ident(ident.to_string())
    }
}

impl quote::ToTokens for Ident {
    fn to_tokens(&self, tokens: &mut TokenStream) {
        tokens.append(proc_macro2::Ident::new(&self.0, proc_macro2::Span::call_site()))
    }
}

impl std::fmt::Display for Ident {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

#[derive(Clone, Debug)]
pub struct Segments {
    pub segments: Vec<Ident>,
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct InnerType {
    pub path: Vec<Ident>,
    pub ident: Ident,
    pub args: Option<Box<InnerType>>,
}

impl InnerType {
    pub fn new(ident: &str) -> Self {
        Self {
            path: vec![],
            ident: Ident::new(ident),
            args: None,
        }
    }
}

/// Token type. A rust AST token, representing a type.
#[derive(Clone, Debug, PartialEq, Hash)]
pub enum TType {
    Option(Box<TType>),
    Vec(Box<TType>),
    /// Database primitive, includes DateTime, Jsonb, etc.
    Inner(InnerType),
    Join(Box<TType>),
}

impl TType {
    pub fn joined_type(&self) -> Option<&TType> {
        match &self {
            TType::Join(ty) => Some(ty.as_ref()),
            _ => None,
        }
    }

    pub fn is_string(&self) -> bool {
        match self {
            TType::Inner(ty) => ty.ident.0 == "String",
            _ => false,
        }
    }

    pub fn is_json(&self) -> bool {
        match self {
            TType::Inner(ty) => ty.ident.0 == "Json",
            TType::Option(ty) => ty.is_json(),
            _ => false,
        }
    }

    pub fn is_join(&self) -> bool {
        matches!(self, TType::Join(_))
    }

    pub fn is_option(&self) -> bool {
        matches!(self, TType::Option(_))
    }

    pub fn inner_type_name(&self) -> String {
        match self {
            TType::Inner(ty) => ty.ident.0.to_string(),
            TType::Option(ty) => ty.inner_type_name(),
            TType::Vec(ty) => ty.inner_type_name(),
            TType::Join(ty) => ty.inner_type_name(),
        }
    }

    pub fn inner_type_mut(&mut self) -> &mut InnerType {
        match self {
            TType::Inner(ty) => ty,
            TType::Option(ty) => ty.inner_type_mut(),
            TType::Vec(ty) => ty.inner_type_mut(),
            TType::Join(ty) => ty.inner_type_mut(),
        }
    }

    pub fn inner_type(&self) -> &InnerType {
        match self {
            TType::Inner(ty) => ty,
            TType::Option(ty) => ty.inner_type(),
            TType::Vec(ty) => ty.inner_type(),
            TType::Join(ty) => ty.inner_type(),
        }
    }

    pub fn qualified_inner_name(&self) -> TokenStream {
        match self {
            TType::Inner(ty) => {
                let segments = ty.path.iter();
                let ident = &ty.ident;
                quote::quote! {
                    #(#segments)::* #ident
                }
            }
            TType::Option(ty) => ty.qualified_inner_name(),
            TType::Vec(ty) => ty.qualified_inner_name(),
            TType::Join(ty) => ty.qualified_inner_name(),
        }
    }
}

impl From<&syn::Path> for InnerType {
    fn from(path: &syn::Path) -> Self {
        let segment = path.segments.last().expect("path must have at least one segment");
        let args: Option<Box<InnerType>> = if let PathArguments::AngleBracketed(args) = &segment.arguments {
            let args = &args.args;
            let syn::GenericArgument::Type(ty) = args.first().unwrap() else {
                panic!("Option must have a type parameter");
            };
            let syn::Type::Path(path) = &ty else {
                panic!("Option must have a type parameter");
            };
            Some(Box::new(InnerType::from(&path.path)))
        } else {
            None
        };
        let mut path = path.segments.iter().map(|s| Ident::from(&s.ident)).collect::<Vec<_>>();
        let ident = path.pop().expect("path must have at least one segment");
        InnerType {
            path,
            args,
            ident,
        }
    }
}

impl From<InnerType> for TType {
    fn from(value: InnerType) -> Self {
        match value.ident.0.as_str() {
            "Option" => {
                let ty = value.args.unwrap();
                TType::Option(Box::new(TType::from(*ty)))
            }
            "Vec" => {
                let ty = value.args.unwrap();
                TType::Vec(Box::new(TType::from(*ty)))
            }
            "Join" => {
                let ty = value.args.unwrap();
                TType::Join(Box::new(TType::from(*ty)))
            }
            _ => TType::Inner(value),
        }
    }
}

impl From<&syn::Path> for TType {
    fn from(path: &syn::Path) -> Self {
        let other = InnerType::from(path);
        TType::from(other)
    }
}

impl quote::ToTokens for InnerType {
    fn to_tokens(&self, tokens: &mut TokenStream) {
        let args = if let Some(args) = &self.args {
            quote::quote! { <#args> }
        } else {
            quote::quote! {}
        };
        let path = &self.path;
        let ident = &self.ident;
        tokens.append_all(quote::quote! { #(#path ::)* #ident #args });
    }
}

impl quote::ToTokens for TType {
    fn to_tokens(&self, tokens: &mut TokenStream) {
        match self {
            TType::Option(ty) => {
                tokens.append_all(quote::quote! { Option<#ty> });
            }
            TType::Vec(ty) => {
                tokens.append_all(quote::quote! { Vec<#ty> });
            }
            TType::Inner(ty) => {
                ty.to_tokens(tokens);
            }
            TType::Join(ty) => {
                tokens.append_all(quote::quote! { ormlite::model::Join<#ty> });
            }
        }
    }
}

/// Metadata used for IntoArguments, TableMeta, and (subset of) Model
#[derive(Builder, Debug, Clone)]
pub struct TableMetadata {
    pub table_name: String,
    pub struct_name: Ident,
    pub columns: Vec<ColumnMetadata>,
    pub databases: Vec<String>,

    /// If you're using this, consider whether you should be using a ModelMetadata and its pkey,
    /// which is not optional, instead.
    pub pkey: Option<String>,
}

/// Metadata used for IntoArguments, TableMeta, and (subset of) Model
#[derive(Builder, Debug, Clone)]
pub struct ModelMetadata {
    pub inner: TableMetadata,
    pub insert_struct: Option<String>,
    pub pkey: ColumnMetadata,
}

// impl Default for TableMetadata {
//     fn default() -> Self {
//         TableMetadata {
//             table_name: String::new(),
//             struct_name: Ident::new(""),
//             pkey: None,
//             columns: vec![],
//             insert_struct: None,
//             databases: vec![],
//         }
//     }
// }
impl ModelMetadata {
    pub fn new(name: &str, columns: Vec<ColumnMetadata>) -> Self {
        let inner = TableMetadata::new(name, columns);
        Self {
            pkey: inner.columns.iter()
                .find(|c| c.column_name == "id")
                .unwrap()
                .clone(),
            inner,
            insert_struct: None,
        }
    }
    pub fn table(&self) -> &str {
        &self.inner.table_name
    }

    pub fn struct_name(&self) -> &Ident {
        &self.inner.struct_name
    }

    pub fn builder_struct(&self) -> Ident {
        let mut s = self.inner.struct_name.0.clone();
        s.push_str("Builder");
        Ident(s)
    }

    pub fn database_columns_except_pkey(&self) -> impl Iterator<Item=&ColumnMetadata> + '_ {
        self.inner.columns.iter()
            .filter(|&c| !c.skip)
            .filter(|&c| self.pkey.column_name != c.column_name)
    }

    pub fn database_columns(&self) -> impl Iterator<Item=&ColumnMetadata> + '_ {
        self.inner.database_columns()
    }

    pub fn many_to_one_joins(&self) -> impl Iterator<Item=&ColumnMetadata> + '_ {
        self.inner.many_to_one_joins()
    }

    pub fn columns(&self) -> impl Iterator<Item=&ColumnMetadata> + '_ {
        self.inner.columns.iter()
    }

    pub fn from_derive(ast: &DeriveInput) -> Result<Self, SyndecodeError> {
        let meta = TableMetadata::from_derive(ast)?;
        let pkey = meta.pkey.clone().expect(&format!(
            "No column marked with #[ormlite(primary_key)], and no column named id, uuid, {0}_id, or {0}_uuid",
            meta.table_name,
        ));
        let pkey = meta.columns.iter().find(|&c| c.column_name == pkey).unwrap().clone();
        let mut insert_struct = None;
        for attr in ast.attrs.iter().filter(|a| a.path().is_ident("ormlite")) {
            let args: ModelAttributes = attr.parse_args()
                .map_err(|e| SyndecodeError(e.to_string()))?;
            if let Some(value) = args.insertable {
                insert_struct = Some(value.to_string());
            }
        }
        Ok(Self {
            inner: meta,
            insert_struct,
            pkey,
        })
    }
}

impl TableMetadata {
    pub fn new(name: &str, columns: Vec<ColumnMetadata>) -> Self {
        TableMetadata {
            table_name: name.to_string(),
            struct_name: Ident(name.to_case(Case::Pascal)),
            pkey: None,
            columns,
            databases: vec![],
        }
    }


    pub fn builder() -> TableMetadataBuilder {
        TableMetadataBuilder::default()
    }

    pub fn from_derive(ast: &DeriveInput) -> Result<TableMetadata, SyndecodeError> {
        let mut databases = vec![];
        let struct_name = Ident::from(&ast.ident);
        let mut table_name = None;
        for attr in ast.attrs.iter().filter(|a| a.path().is_ident("ormlite")) {
            let args: ModelAttributes = attr.parse_args()
                .map_err(|e| SyndecodeError(e.to_string()))?;
            if let Some(value) = args.table {
                table_name = Some(value.value());
            }
            if let Some(value) = args.database {
                databases.push(value.value());
            }
        }
        let table_name = table_name.unwrap_or_else(|| struct_name.to_string().to_case(Case::Snake));
        let mut columns = ast.fields()
            .map(ColumnMetadata::try_from)
            .collect::<Result<Vec<_>, _>>().unwrap();
        let mut pkey = columns
            .iter()
            .find(|&c| c.marked_primary_key).map(|c| c.clone())
            .map(|c| c.column_name.clone());
        if pkey.is_none() {
            let candidates = sqlmo::util::pkey_column_names(&table_name);
            let c = columns.iter_mut()
                .find(|c| candidates.contains(c.identifier.as_ref()));
            if let Some(c) = c {
                c.has_database_default = true;
                pkey = Some(c.column_name.clone());
            }
        }
        Ok(Self {
            table_name,
            struct_name,
            columns,
            databases,
            pkey,
        })
    }

    pub fn all_fields(&self) -> impl Iterator<Item=&Ident> + '_ {
        self.columns.iter()
            .map(|c| &c.identifier)
    }

    pub fn database_columns(&self) -> impl Iterator<Item=&ColumnMetadata> + '_ {
        self.columns.iter()
            .filter(|&c| !c.skip)
    }

    pub fn many_to_one_joins(&self) -> impl Iterator<Item=&ColumnMetadata> + '_ {
        self.columns.iter()
            .filter(|&c| c.many_to_one_column_name.is_some())
    }
}


#[derive(Clone, Debug)]
pub struct ForeignKey {
    pub model: String,
    pub column: String,
}

/// All the metadata we can capture about a column
#[derive(Clone, Debug, Builder)]
pub struct ColumnMetadata {
    /// Name of the column in the database
    pub column_name: String,
    pub column_type: TType,
    /// Only says whether the primary key is marked (with an attribute). Use table_metadata.primary_key to definitively know the primary key.
    pub marked_primary_key: bool,
    pub has_database_default: bool,
    /// Identifier used in Rust to refer to the column
    pub identifier: Ident,

    // only for joins. Database key
    pub many_to_one_column_name: Option<String>,
    pub many_to_many_table: Option<String>,
    pub one_to_many_foreign_key: Option<ForeignKey>,

    pub skip: bool,
    pub experimental_encode_as_json: bool,
    pub rust_default: Option<String>,
}

impl ColumnMetadata {
    pub fn is_default(&self) -> bool {
        self.rust_default.is_some() || self.has_database_default
    }
}

impl Default for ColumnMetadata {
    fn default() -> Self {
        Self {
            column_name: String::new(),
            column_type: TType::Inner(InnerType::new("String")),
            marked_primary_key: false,
            has_database_default: false,
            identifier: Ident::new("column"),
            many_to_one_column_name: None,
            many_to_many_table: None,
            one_to_many_foreign_key: None,
            skip: false,
            experimental_encode_as_json: false,
            rust_default: None,
        }
    }
}

impl ColumnMetadata {
    pub fn new(name: &str, ty: &str) -> Self {
        Self {
            column_name: name.to_string(),
            column_type: TType::Inner(InnerType::new(ty)),
            ..Self::default()
        }
    }

    pub fn new_join(name: &str, join_model: &str) -> Self {
        Self {
            column_name: name.to_string(),
            column_type: TType::Join(Box::new(TType::Inner(InnerType::new(join_model)))),
            ..Self::default()
        }
    }

    pub fn builder() -> ColumnMetadataBuilder {
        ColumnMetadataBuilder::default()
    }

    pub fn is_join(&self) -> bool {
        matches!(self.column_type, TType::Join(_))
    }

    pub fn is_join_many(&self) -> bool {
        let TType::Join(join) = &self.column_type else {
            return false;
        };
        let TType::Inner(o) = join.as_ref() else {
            return false;
        };
        o.ident.0 == "Vec"
    }

    pub fn is_json(&self) -> bool {
        self.column_type.is_json()
    }

    /// We expect this to only return a `Model` of some kind.
    pub fn joined_struct_name(&self) -> Option<String> {
        let TType::Join(join) = &self.column_type else {
            return None;
        };
        Some(join.inner_type_name())
    }

    pub fn joined_model(&self) -> TokenStream {
        self.column_type.qualified_inner_name()
    }
}


impl TryFrom<&Field> for ColumnMetadata {
    type Error = SyndecodeError;

    fn try_from(f: &Field) -> Result<Self, Self::Error> {
        let mut builder = ColumnMetadata::builder();
        let ident = f.ident.as_ref().expect("No ident on field");
        let syn::Type::Path(ty) = &f.ty else {
            return Err(SyndecodeError(format!("No type on field {}", ident)));
        };
        let ty = TType::from(&ty.path);
        let is_join = ty.is_join();
        builder
            .column_name(ident.to_string())
            .identifier(Ident::from(ident))
            .column_type(ty)
            .marked_primary_key(false)
            .has_database_default(false)
            .many_to_one_column_name(None)
            .many_to_many_table(None)
            .one_to_many_foreign_key(None)
            .skip(false)
            .experimental_encode_as_json(false)
            .rust_default(None)
        ;
        let mut has_join_directive = false;
        for attr in f.attrs.iter().filter(|&a| a.path().is_ident("ormlite")) {
            let args: ColumnAttributes = attr.parse_args().unwrap();
            if args.primary_key.value() {
                builder.marked_primary_key(true);
                builder.has_database_default(true);
            }
            if args.default.value() {
                builder.has_database_default(true);
            }
            if let Some(column_name) = args.column {
                let column_name = column_name.value();
                builder.column_name(column_name);
            }
            if let Some(value) = args.join_column {
                let value = value.value.value();
                builder.many_to_one_column_name(Some(value.to_string()));
                builder.column_name(value);
                has_join_directive = true;
            }
            if let Some(path) = args.many_to_many_table {
                let value = path.to_string();
                builder.many_to_many_table(Some(value));
                has_join_directive = true;
            }
            if let Some(_path) = args.one_to_many_foreign_key {
                panic!("Join support in ormlite is in alpha state, and one_to_many_foreign_key is unfortunately not implemented yet.");
                // builder.one_to_many_foreign_key(Some(ForeignKey::from(&path)));
                // has_join_directive = true;
            }
            if args.skip.value() {
                builder.skip(true);
            }
            if args.experimental_encode_as_json.value() {
                builder.experimental_encode_as_json(true);
            }
            if let Some(default_value) = args.default_value {
                builder.rust_default(Some(default_value.value.value()));
            }
            if args.insertable_primary_key.value() {
                builder.marked_primary_key(true);
            }
        }
        if is_join && !has_join_directive {
            return Err(SyndecodeError(format!("Column {ident} is a Join. You must specify one of these attributes: join_column (for many to one), many_to_many_table_name, or one_to_many_foreign_key {is_join} {has_join_directive} {f:?}")));
        }
        builder.build().map_err(|e| SyndecodeError(e.to_string()))
    }
}


#[cfg(test)]
mod test {
    use syn::ItemStruct;
    use super::*;

    #[test]
    fn test_primitive() {
        use syn::Path;
        let ty = TType::from(&syn::parse_str::<Path>("i32").unwrap());
        assert!(!ty.is_json());

        let ty = TType::from(&syn::parse_str::<Path>("Json<User>").unwrap());
        assert!(ty.is_json());
    }

    #[test]
    fn test_other_type_to_quote() {
        use syn::Path;
        let ty = TType::from(&syn::parse_str::<Path>("rust_decimal::Decimal").unwrap());
        let TType::Inner(ty) = &ty else {
            panic!("expected primitive");
        };
        assert_eq!(ty.ident.0, "Decimal");
        assert_eq!(ty.path.len(), 1);
        assert_eq!(ty.path[0].0.as_str(), "rust_decimal");
        let z = quote::quote!(#ty);
        assert_eq!(z.to_string(), "rust_decimal :: Decimal");
    }

    #[test]
    fn test_decode_metadata() {
        let ast = syn::parse_str::<ItemStruct>(r#"struct User {
            #[ormlite(column = "Id")]
            id: i32,
        }"#).unwrap();
        let input = DeriveInput::from(ast);
        let meta = ModelMetadata::from_derive(&input).unwrap();
        assert!(meta.pkey.column_name == "Id");
    }
}
