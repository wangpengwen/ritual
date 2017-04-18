use cpp_ffi_data::CppAndFfiMethod;
use cpp_type::CppType;
use common::errors::{Result, unexpected};
use common::string_utils::JoinWithSeparator;
use common::utils::MapIfOk;
use rust_type::{RustName, CompleteType, RustType, RustTypeIndirection};
use cpp_method::CppMethodDoc;
use cpp_data::CppTypeDoc;
use std::collections::{HashMap, HashSet};
use std::path::PathBuf;

/// One variant of a Rust enum
#[derive(Debug, PartialEq, Eq, Clone)]
#[derive(Serialize, Deserialize)]
pub struct RustEnumValue {
  /// Identifier
  pub name: String,
  /// Corresponding value
  pub value: i64,
  /// Documentation of corresponding C++ variants
  pub cpp_docs: Vec<CppEnumValueDocItem>,
  /// True if this variant was added because enums with
  /// one variant are not supported
  pub is_dummy: bool,
}


/// C++ documentation data for a enum variant
#[derive(Debug, PartialEq, Eq, Clone)]
#[derive(Serialize, Deserialize)]
pub struct CppEnumValueDocItem {
  /// C++ name of the variant
  pub variant_name: String,
  /// HTML content
  pub doc: Option<String>,
}


#[derive(Debug, PartialEq, Eq, Clone)]
#[derive(Serialize, Deserialize)]
pub struct RustQtSlotWrapper {
  pub arguments: Vec<CompleteType>,
  pub receiver_id: String,
  pub public_type_name: String,
  pub callback_name: String,
}

/// Information about a Rust type wrapper
#[derive(Debug, PartialEq, Eq, Clone)]
#[derive(Serialize, Deserialize)]
pub enum RustTypeWrapperKind {
  /// Enum wrapper
  Enum {
    /// List of enum values
    values: Vec<RustEnumValue>,
    /// True if `FlaggableEnum` trait is implemented
    /// for this type, i.e. if `QFlags<T>` with this C++ type
    /// is used in API.
    is_flaggable: bool,
  },
  /// Struct wrapper
  Struct {
    /// Name of the constant containing size of the corresponding
    /// C++ type in bytes. Value of the constant is determined at
    /// crate compile time.
    /// If None, this struct can only be used as pointer, like an
    /// empty enum.
    size_const_name: Option<String>,
    /// True if `CppDeletable` trait is implemented
    /// for this type, i.e. if this C++ type has public destructor.
    is_deletable: bool,
    slot_wrapper: Option<RustQtSlotWrapper>,
  },
}

/// Exported information about a Rust wrapper type
#[derive(Debug, Clone)]
#[derive(Serialize, Deserialize)]
pub struct RustProcessedTypeInfo {
  /// Full name of corresponding C++ type (class or enum).
  pub cpp_name: String,
  /// C++ documentation for this type
  pub cpp_doc: Option<CppTypeDoc>,
  /// Template arguments. None if C++ type is not a template class.
  pub cpp_template_arguments: Option<Vec<CppType>>,
  /// Kind of the type and additional information.
  pub kind: RustTypeWrapperKind,
  /// Identifier of Rust type
  pub rust_name: RustName,
  /// Indicates whether this type is public
  pub is_public: bool,
}



/// Exported information about generated crate
#[derive(Debug, Clone)]
#[derive(Serialize, Deserialize)]
pub struct RustExportInfo {
  /// Name of the crate
  pub crate_name: String,
  /// Version of the crate
  pub crate_version: String,
  /// Directory with the generated crate
  pub output_path: String,
  /// List of generated types
  pub rust_types: Vec<RustProcessedTypeInfo>,
}


#[derive(Debug, PartialEq, Eq, Clone)]
pub struct RustMethodDocItem {
  pub doc: Option<CppMethodDoc>,
  pub rust_fns: Vec<String>,
  pub cpp_fn: String,
  pub rust_cross_references: Vec<RustCrossReference>,
}



#[derive(Debug, PartialEq, Eq, Clone)]
pub enum RustMethodScope {
  Impl { target_type: RustType },
  TraitImpl,
  Free,
}

#[derive(Debug, PartialEq, Eq, Clone)]
pub struct RustMethodArgument {
  pub argument_type: CompleteType,
  pub name: String,
  pub ffi_index: Option<i32>,
}

#[derive(Debug, PartialEq, Eq, Clone)]
pub struct RustMethodArgumentsVariant {
  pub arguments: Vec<RustMethodArgument>,
  pub cpp_method: CppAndFfiMethod,
  pub return_type_ffi_index: Option<i32>,
  pub return_type: CompleteType,
}

// impl RustMethodArgumentsVariant {
//  pub fn has_unportable_arg_types(&self) -> bool {
//    self.arguments.iter().any(|arg| arg.argument_type.cpp_type.is_platform_dependent())
//  }
// }

#[derive(Debug, PartialEq, Eq, Clone)]
#[allow(dead_code)]
pub enum RustMethodArguments {
  SingleVariant(RustMethodArgumentsVariant),
  MultipleVariants {
    params_trait_name: String,
    params_trait_lifetime: Option<String>,
    params_trait_return_type: Option<RustType>,
    shared_arguments: Vec<RustMethodArgument>,
    variant_argument_name: String,
    cpp_method_name: String,
  },
}

#[derive(Debug, PartialEq, Eq, Clone)]
pub struct RustMethod {
  pub scope: RustMethodScope,
  pub is_unsafe: bool,
  pub name: RustName,
  pub arguments: RustMethodArguments,
  pub docs: Vec<RustMethodDocItem>,
}

#[derive(Debug, PartialEq, Eq, Clone)]
pub struct RustSingleMethod {
  pub scope: RustMethodScope,
  pub is_unsafe: bool,
  pub name: RustName,
  pub arguments: RustMethodArgumentsVariant,
  pub doc: Option<RustMethodDocItem>,
}


#[derive(Debug, PartialEq, Eq, Clone, Hash)]
pub enum RustMethodSelfArgKind {
  Static,
  ConstRef,
  MutRef,
  Value,
}

fn detect_self_arg_kind(args: &[RustMethodArgument]) -> Result<RustMethodSelfArgKind> {
  Ok(if let Some(arg) = args.get(0) {
       if arg.name == "self" {
         if let RustType::Common {
                  ref indirection,
                  ref is_const,
                  ..
                } = arg.argument_type.rust_api_type {
           match *indirection {
             RustTypeIndirection::Ref { .. } => {
               if *is_const {
                 RustMethodSelfArgKind::ConstRef
               } else {
                 RustMethodSelfArgKind::MutRef
               }
             }
             RustTypeIndirection::None => RustMethodSelfArgKind::Value,
             _ => return Err(unexpected("invalid self argument type").into()),
           }
         } else {
           return Err(unexpected("invalid self argument type").into());
         }
       } else {
         RustMethodSelfArgKind::Static
       }
     } else {
       RustMethodSelfArgKind::Static
     })
}

impl RustMethod {
  //  pub fn self_arg_kind(&self) -> Result<RustMethodSelfArgKind> {
  //    let args = match self.arguments {
  //      RustMethodArguments::SingleVariant(ref var) => &var.arguments,
  //      RustMethodArguments::MultipleVariants { ref shared_arguments, .. } => shared_arguments,
  //    };
  //    detect_self_arg_kind(args)
  //  }

  #[allow(dead_code)]
  pub fn cpp_cross_references(&self) -> Vec<String> {
    let mut r = Vec::new();
    for doc in &self.docs {
      if let Some(ref doc) = doc.doc {
        r.append(&mut doc.cross_references.clone());
      }
    }
    r
  }

  #[allow(dead_code)]
  pub fn add_rust_cross_references(&mut self, table: HashMap<String, RustCrossReference>) {
    for doc in &mut self.docs {
      let mut result = Vec::new();
      if let Some(ref doc) = doc.doc {
        for reference in &doc.cross_references {
          if let Some(r) = table.get(reference) {
            result.push(r.clone());
          }
        }
      }
      doc.rust_cross_references = result;
    }
  }
}

impl RustSingleMethod {
  pub fn to_rust_method(&self) -> RustMethod {
    RustMethod {
      name: self.name.clone(),
      arguments: RustMethodArguments::SingleVariant(self.arguments.clone()),
      docs: if let Some(ref doc) = self.doc {
        vec![doc.clone()]
      } else {
        Vec::new()
      },
      is_unsafe: self.is_unsafe,
      scope: self.scope.clone(),
    }
  }

  pub fn self_arg_kind(&self) -> Result<RustMethodSelfArgKind> {
    detect_self_arg_kind(&self.arguments.arguments)
  }

  pub fn can_be_overloaded_with(&self, other_method: &RustSingleMethod) -> Result<bool> {
    if self.is_unsafe != other_method.is_unsafe {
      return Ok(false);
    }
    if self.self_arg_kind()? != other_method.self_arg_kind()? {
      return Ok(false);
    }
    if self.arguments.arguments.len() == other_method.arguments.arguments.len() {
      if self
           .arguments
           .arguments
           .iter()
           .zip(other_method.arguments.arguments.iter())
           .all(|(arg1, arg2)| {
                  arg1
                    .argument_type
                    .cpp_type
                    .can_be_the_same_as(&arg2.argument_type.cpp_type) &&
                  !(arg1.name == "allocation_place_marker" &&
                    arg2.name == "allocation_place_marker" && arg1 != arg2)
                }) {
        return Ok(false);
      }
    }
    Ok(true)
  }

  pub fn name_suffix(&self,
                     caption_strategy: &RustMethodCaptionStrategy,
                     all_self_args: &HashSet<RustMethodSelfArgKind>,
                     index: usize)
                     -> Result<Option<String>> {
    if caption_strategy == &RustMethodCaptionStrategy::UnsafeOnly {
      return Ok(if self.is_unsafe {
                  Some("unsafe".to_string())
                } else {
                  None
                });
    }
    let result = {
      let self_arg_kind = self.self_arg_kind()?;
      let self_arg_kind_caption = if all_self_args.len() == 1 ||
                                     self_arg_kind == RustMethodSelfArgKind::ConstRef {
        None
      } else if self_arg_kind == RustMethodSelfArgKind::Static {
        Some("static")
      } else if self_arg_kind == RustMethodSelfArgKind::MutRef {
        if all_self_args.contains(&RustMethodSelfArgKind::ConstRef) {
          Some("mut")
        } else {
          None
        }
      } else {
        return Err("unsupported self arg kinds combination".into());
      };
      let other_caption = match *caption_strategy {
        RustMethodCaptionStrategy::NoCaption => None,
        RustMethodCaptionStrategy::UnsafeOnly => unreachable!(),
        RustMethodCaptionStrategy::Index => Some(index.to_string()),
        RustMethodCaptionStrategy::ArgNames => {
          if self.arguments.arguments.is_empty() {
            Some("no_args".to_string())
          } else {
            Some(self
                   .arguments
                   .arguments
                   .iter()
                   .map(|a| &a.name)
                   .join("_"))
          }
        }
        RustMethodCaptionStrategy::ArgTypes => {
          let context = match self.scope {
            RustMethodScope::Free => &self.name,
            RustMethodScope::Impl { ref target_type } => {
              if let RustType::Common { ref base, .. } = *target_type {
                base
              } else {
                return Err("unexpected uncommon Rust type".into());
              }
            }
            RustMethodScope::TraitImpl => {
              return Err("can't generate Rust method caption for a trait impl method".into())
            }
          };

          if self.arguments.arguments.is_empty() {
            Some("no_args".to_string())
          } else {
            Some(self
                   .arguments
                   .arguments
                   .iter()
                   .filter(|t| &t.name != "self")
                   .map_if_ok(|t| t.argument_type.rust_api_type.caption(context))?
                   .join("_"))
          }
        }
      };
      let mut key_caption_items = Vec::new();
      if let Some(c) = self_arg_kind_caption {
        key_caption_items.push(c.to_string());
      }
      if let Some(c) = other_caption {
        key_caption_items.push(c);
      }
      if key_caption_items.is_empty() {
        None
      } else {
        Some(key_caption_items.join("_"))
      }
    };
    Ok(result)
  }
}

#[derive(Debug, PartialEq, Eq, Clone)]
pub enum TraitImplExtra {
  CppDeletable { deleter_name: String },
}

#[derive(Debug, PartialEq, Eq, Clone)]
pub struct TraitAssociatedType {
  pub name: String,
  pub value: RustType,
}

#[derive(Debug, PartialEq, Eq, Clone)]
pub struct TraitImpl {
  pub target_type: RustType,
  pub associated_types: Vec<TraitAssociatedType>,
  pub trait_type: RustType,
  pub extra: Option<TraitImplExtra>,
  pub methods: Vec<RustMethod>,
}

#[allow(dead_code)]
#[derive(Debug, PartialEq, Eq, Clone)]
pub enum RustCrossReferenceKind {
  Method { scope: RustMethodScope },
  Type,
}

#[derive(Debug, PartialEq, Eq, Clone)]
pub struct RustCrossReference {
  name: RustName,
  kind: RustCrossReferenceKind,
}

#[derive(Debug, PartialEq, Eq, Clone)]
pub enum RustQtReceiverType {
  Signal,
  Slot,
}

#[derive(Debug, PartialEq, Eq, Clone)]
pub struct RustQtReceiverDeclaration {
  pub type_name: String,
  pub method_name: String,
  pub receiver_type: RustQtReceiverType,
  pub receiver_id: String,
  pub arguments: Vec<RustType>,
}

#[derive(Debug, PartialEq, Eq, Clone)]
pub enum RustTypeDeclarationKind {
  CppTypeWrapper {
    kind: RustTypeWrapperKind,
    cpp_type_name: String,
    cpp_template_arguments: Option<Vec<CppType>>,
    cpp_doc: Option<CppTypeDoc>,
    rust_cross_references: Vec<RustCrossReference>,
    methods: Vec<RustMethod>,
    trait_impls: Vec<TraitImpl>,
    qt_receivers: Vec<RustQtReceiverDeclaration>,
  },
  MethodParametersTrait {
    lifetime: Option<String>,
    shared_arguments: Vec<RustMethodArgument>,
    return_type: Option<RustType>,
    impls: Vec<RustMethodArgumentsVariant>,
    method_scope: RustMethodScope,
    method_name: RustName,
    is_unsafe: bool,
  },
}

#[derive(Debug, PartialEq, Eq, Clone)]
pub struct RustTypeDeclaration {
  pub is_public: bool,
  pub name: RustName,
  pub kind: RustTypeDeclarationKind,
}

#[derive(Debug, PartialEq, Eq, Clone)]
pub struct RustModule {
  pub name: String,
  pub types: Vec<RustTypeDeclaration>,
  pub functions: Vec<RustMethod>,
  pub trait_impls: Vec<TraitImpl>,
  pub submodules: Vec<RustModule>,
}

#[derive(Debug, Clone)]
pub struct DependencyInfo {
  pub rust_export_info: RustExportInfo,
  pub cache_path: PathBuf,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RustMethodCaptionStrategy {
  NoCaption,
  UnsafeOnly,
  ArgTypes,
  ArgNames,
  Index,
}
impl RustMethodCaptionStrategy {
  pub fn all() -> &'static [RustMethodCaptionStrategy] {
    use self::RustMethodCaptionStrategy::*;
    const LIST: &'static [RustMethodCaptionStrategy] = &[NoCaption, UnsafeOnly, ArgTypes,
                                                         ArgNames, Index];
    return LIST;
  }
}
