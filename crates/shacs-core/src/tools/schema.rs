use serde_json::{json, Map, Value};

pub trait SchemaFragment {
    fn to_json_schema(&self) -> Value;
}

#[derive(Debug, Clone, Default)]
pub struct StringSchema {
    description: String,
    min_length: Option<u64>,
    max_length: Option<u64>,
    enum_values: Option<Vec<Value>>,
    nullable: bool,
}

impl StringSchema {
    pub fn new(description: impl Into<String>) -> Self {
        Self {
            description: description.into(),
            min_length: None,
            max_length: None,
            enum_values: None,
            nullable: false,
        }
    }

    pub fn min_length(mut self, value: u64) -> Self {
        self.min_length = Some(value);
        self
    }

    pub fn max_length(mut self, value: u64) -> Self {
        self.max_length = Some(value);
        self
    }

    pub fn enum_values(mut self, values: impl IntoIterator<Item = Value>) -> Self {
        self.enum_values = Some(values.into_iter().collect());
        self
    }

    pub fn nullable(mut self) -> Self {
        self.nullable = true;
        self
    }
}

impl SchemaFragment for StringSchema {
    fn to_json_schema(&self) -> Value {
        let mut object = Map::new();
        object.insert("type".to_owned(), nullable_type("string", self.nullable));
        add_description(&mut object, &self.description);
        if let Some(value) = self.min_length {
            object.insert("minLength".to_owned(), value.into());
        }
        if let Some(value) = self.max_length {
            object.insert("maxLength".to_owned(), value.into());
        }
        if let Some(values) = &self.enum_values {
            object.insert("enum".to_owned(), Value::Array(values.clone()));
        }
        Value::Object(object)
    }
}

#[derive(Debug, Clone, Default)]
pub struct IntegerSchema {
    description: String,
    minimum: Option<i64>,
    maximum: Option<i64>,
    enum_values: Option<Vec<i64>>,
    nullable: bool,
}

impl IntegerSchema {
    pub fn new(description: impl Into<String>) -> Self {
        Self {
            description: description.into(),
            minimum: None,
            maximum: None,
            enum_values: None,
            nullable: false,
        }
    }

    pub fn minimum(mut self, value: i64) -> Self {
        self.minimum = Some(value);
        self
    }

    pub fn maximum(mut self, value: i64) -> Self {
        self.maximum = Some(value);
        self
    }

    pub fn enum_values(mut self, values: impl IntoIterator<Item = i64>) -> Self {
        self.enum_values = Some(values.into_iter().collect());
        self
    }

    pub fn nullable(mut self) -> Self {
        self.nullable = true;
        self
    }
}

impl SchemaFragment for IntegerSchema {
    fn to_json_schema(&self) -> Value {
        let mut object = Map::new();
        object.insert("type".to_owned(), nullable_type("integer", self.nullable));
        add_description(&mut object, &self.description);
        if let Some(value) = self.minimum {
            object.insert("minimum".to_owned(), value.into());
        }
        if let Some(value) = self.maximum {
            object.insert("maximum".to_owned(), value.into());
        }
        if let Some(values) = &self.enum_values {
            object.insert(
                "enum".to_owned(),
                Value::Array(values.iter().copied().map(Value::from).collect()),
            );
        }
        Value::Object(object)
    }
}

#[derive(Debug, Clone, Default)]
pub struct NumberSchema {
    description: String,
    minimum: Option<f64>,
    maximum: Option<f64>,
    nullable: bool,
}

impl NumberSchema {
    pub fn new(description: impl Into<String>) -> Self {
        Self {
            description: description.into(),
            ..Self::default()
        }
    }

    pub fn minimum(mut self, value: f64) -> Self {
        self.minimum = Some(value);
        self
    }

    pub fn maximum(mut self, value: f64) -> Self {
        self.maximum = Some(value);
        self
    }

    pub fn nullable(mut self) -> Self {
        self.nullable = true;
        self
    }
}

impl SchemaFragment for NumberSchema {
    fn to_json_schema(&self) -> Value {
        let mut object = Map::new();
        object.insert("type".to_owned(), nullable_type("number", self.nullable));
        add_description(&mut object, &self.description);
        if let Some(value) = self.minimum {
            object.insert("minimum".to_owned(), json!(value));
        }
        if let Some(value) = self.maximum {
            object.insert("maximum".to_owned(), json!(value));
        }
        Value::Object(object)
    }
}

#[derive(Debug, Clone, Default)]
pub struct BooleanSchema {
    description: String,
    default: Option<bool>,
    nullable: bool,
}

impl BooleanSchema {
    pub fn new(description: impl Into<String>) -> Self {
        Self {
            description: description.into(),
            default: None,
            nullable: false,
        }
    }

    pub fn default(mut self, value: bool) -> Self {
        self.default = Some(value);
        self
    }

    pub fn nullable(mut self) -> Self {
        self.nullable = true;
        self
    }
}

impl SchemaFragment for BooleanSchema {
    fn to_json_schema(&self) -> Value {
        let mut object = Map::new();
        object.insert("type".to_owned(), nullable_type("boolean", self.nullable));
        add_description(&mut object, &self.description);
        if let Some(value) = self.default {
            object.insert("default".to_owned(), value.into());
        }
        Value::Object(object)
    }
}

pub struct ArraySchema {
    items: Box<dyn SchemaFragment + Send + Sync>,
    description: String,
    min_items: Option<u64>,
    max_items: Option<u64>,
    nullable: bool,
}

impl ArraySchema {
    pub fn new(items: impl SchemaFragment + Send + Sync + 'static) -> Self {
        Self {
            items: Box::new(items),
            description: String::new(),
            min_items: None,
            max_items: None,
            nullable: false,
        }
    }

    pub fn description(mut self, value: impl Into<String>) -> Self {
        self.description = value.into();
        self
    }

    pub fn min_items(mut self, value: u64) -> Self {
        self.min_items = Some(value);
        self
    }

    pub fn max_items(mut self, value: u64) -> Self {
        self.max_items = Some(value);
        self
    }

    pub fn nullable(mut self) -> Self {
        self.nullable = true;
        self
    }
}

impl SchemaFragment for ArraySchema {
    fn to_json_schema(&self) -> Value {
        let mut object = Map::new();
        object.insert("type".to_owned(), nullable_type("array", self.nullable));
        object.insert("items".to_owned(), self.items.to_json_schema());
        add_description(&mut object, &self.description);
        if let Some(value) = self.min_items {
            object.insert("minItems".to_owned(), value.into());
        }
        if let Some(value) = self.max_items {
            object.insert("maxItems".to_owned(), value.into());
        }
        Value::Object(object)
    }
}

#[derive(Default)]
pub struct ObjectSchema {
    properties: Map<String, Value>,
    required: Vec<String>,
    description: String,
    additional_properties: Option<Value>,
    nullable: bool,
}

impl ObjectSchema {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn property(mut self, name: impl Into<String>, fragment: impl SchemaFragment) -> Self {
        self.properties
            .insert(name.into(), fragment.to_json_schema());
        self
    }

    pub fn raw_property(mut self, name: impl Into<String>, fragment: Value) -> Self {
        self.properties.insert(name.into(), fragment);
        self
    }

    pub fn required(mut self, fields: impl IntoIterator<Item = impl Into<String>>) -> Self {
        self.required = fields.into_iter().map(Into::into).collect();
        self
    }

    pub fn description(mut self, value: impl Into<String>) -> Self {
        self.description = value.into();
        self
    }

    pub fn additional_properties(mut self, value: Value) -> Self {
        self.additional_properties = Some(value);
        self
    }

    pub fn nullable(mut self) -> Self {
        self.nullable = true;
        self
    }
}

impl SchemaFragment for ObjectSchema {
    fn to_json_schema(&self) -> Value {
        let mut object = Map::new();
        object.insert("type".to_owned(), nullable_type("object", self.nullable));
        object.insert(
            "properties".to_owned(),
            Value::Object(self.properties.clone()),
        );
        if !self.required.is_empty() {
            object.insert(
                "required".to_owned(),
                Value::Array(self.required.iter().cloned().map(Value::String).collect()),
            );
        }
        add_description(&mut object, &self.description);
        if let Some(value) = &self.additional_properties {
            object.insert("additionalProperties".to_owned(), value.clone());
        }
        Value::Object(object)
    }
}

pub struct ToolParameters(ObjectSchema);

impl ToolParameters {
    pub fn new() -> Self {
        Self(ObjectSchema::new())
    }

    pub fn property(mut self, name: impl Into<String>, fragment: impl SchemaFragment) -> Self {
        self.0 = self.0.property(name, fragment);
        self
    }

    pub fn raw_property(mut self, name: impl Into<String>, fragment: Value) -> Self {
        self.0 = self.0.raw_property(name, fragment);
        self
    }

    pub fn required(mut self, fields: impl IntoIterator<Item = impl Into<String>>) -> Self {
        self.0 = self.0.required(fields);
        self
    }

    pub fn description(mut self, value: impl Into<String>) -> Self {
        self.0 = self.0.description(value);
        self
    }
}

impl Default for ToolParameters {
    fn default() -> Self {
        Self::new()
    }
}

impl SchemaFragment for ToolParameters {
    fn to_json_schema(&self) -> Value {
        self.0.to_json_schema()
    }
}

pub fn tool_parameters(schema: impl SchemaFragment) -> Value {
    schema.to_json_schema()
}

pub fn tool_parameters_schema(schema: impl SchemaFragment) -> Value {
    schema.to_json_schema()
}

fn nullable_type(name: &str, nullable: bool) -> Value {
    if nullable {
        json!([name, "null"])
    } else {
        Value::String(name.to_owned())
    }
}

fn add_description(object: &mut Map<String, Value>, description: &str) {
    if !description.is_empty() {
        object.insert(
            "description".to_owned(),
            Value::String(description.to_owned()),
        );
    }
}
