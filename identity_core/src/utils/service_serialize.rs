use crate::utils::{Context, ServiceEndpoint};

use serde::{
    de::{self, Deserialize, Deserializer, MapAccess, Visitor},
    ser::{Serialize, SerializeStruct, Serializer},
};

use std::{
    fmt::{self, Formatter},
    str::FromStr,
};

/// The Json fields for the `ServiceEndpoint`.
enum Field {
    Context,
    Type,
    Instances,
}

/// A visitor for the service endpoint values.
struct ServiceEndpointVisitor;

/// A visitor for the service endpoint keys.
struct FieldVisitor;

/// Deserialize logic for the `ServiceEndpoint` type.
impl<'de> Deserialize<'de> for ServiceEndpoint {
    fn deserialize<D>(deserializer: D) -> Result<ServiceEndpoint, D::Error>
    where
        D: Deserializer<'de>,
    {
        deserializer.deserialize_any(ServiceEndpointVisitor)
    }
}

/// Deserialize logic for the `Field` type.
impl<'de> Deserialize<'de> for Field {
    fn deserialize<D>(deserializer: D) -> Result<Field, D::Error>
    where
        D: Deserializer<'de>,
    {
        deserializer.deserialize_any(FieldVisitor)
    }
}

/// Visitor logic for the `ServiceEndpointVisitor` to deserialize the `ServiceEndpoint`.
impl<'de> Visitor<'de> for ServiceEndpointVisitor {
    type Value = ServiceEndpoint;

    fn expecting(&self, formatter: &mut Formatter) -> fmt::Result {
        formatter.write_str("Expecting a string or a Service Endpoint Struct")
    }

    /// If given a &str use this logic to create a `ServiceEndpoint`.
    fn visit_str<E>(self, value: &str) -> Result<ServiceEndpoint, E>
    where
        E: de::Error,
    {
        Ok(ServiceEndpoint {
            context: Context::from_str(value).expect("Unable to deserialize the context"),
            ..Default::default()
        })
    }

    /// given a map, use this logic to create a `ServiceEndpoint`.
    fn visit_map<M>(self, mut map: M) -> Result<ServiceEndpoint, M::Error>
    where
        M: MapAccess<'de>,
    {
        let mut context: Option<String> = None;
        let mut endpoint_type: Option<String> = None;
        let mut instances: Option<Vec<String>> = None;

        while let Some(key) = map.next_key()? {
            match key {
                Field::Context => {
                    if context.is_some() {
                        return Err(de::Error::duplicate_field("@context"));
                    }
                    context = Some(map.next_value()?);
                }
                Field::Type => {
                    if endpoint_type.is_some() {
                        return Err(de::Error::duplicate_field("type"));
                    }
                    endpoint_type = Some(map.next_value()?);
                }
                Field::Instances => {
                    if instances.is_some() {
                        return Err(de::Error::duplicate_field("instances"));
                    }
                    instances = Some(map.next_value()?);
                }
            }
        }

        let context = context.ok_or_else(|| de::Error::missing_field("@context"))?;

        Ok(ServiceEndpoint {
            context: Context::from_str(&context).expect("Unable to deserialize the context into a Service endpoint"),
            endpoint_type,
            instances,
        })
    }
}

/// Visitor logic for the `FieldVisitor` to deserialize the `Field` type.
impl<'de> Visitor<'de> for FieldVisitor {
    type Value = Field;

    fn expecting(&self, formatter: &mut Formatter) -> fmt::Result {
        formatter.write_str("Expected `@context`, `type`, or `instances`")
    }

    /// If given a &str use this logic to create a `Field`.
    fn visit_str<E>(self, value: &str) -> Result<Field, E>
    where
        E: de::Error,
    {
        match value {
            "@context" => Ok(Field::Context),
            "type" => Ok(Field::Type),
            "instances" => Ok(Field::Instances),
            _ => Err(de::Error::unknown_field(value, &[])),
        }
    }
}

/// Serialize the `ServiceEndpoint`.
impl Serialize for ServiceEndpoint {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        if self.instances == None && self.endpoint_type == None {
            self.context.serialize(serializer)
        } else {
            let mut se = serializer.serialize_struct("", 3)?;
            se.serialize_field("@context", &self.context)?;
            se.serialize_field("type", &self.endpoint_type)?;
            se.serialize_field("instances", &self.instances)?;
            se.end()
        }
    }
}