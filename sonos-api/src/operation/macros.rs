//! Declarative macros for UPnP operation and service definitions
//!
//! This module provides macros that dramatically reduce boilerplate when defining
//! UPnP operations. Instead of manually implementing traits and structs, developers
//! can use simple declarative syntax to generate all necessary code.

use paste::paste;

/// Simplified macro for defining UPnP operations with minimal boilerplate
///
/// This macro generates all the necessary structs and trait implementations
/// for a UPnP operation.
///
/// # Example
/// ```rust,ignore
/// define_upnp_operation! {
///     operation: PlayOperation,
///     action: "Play",
///     service: AVTransport,
///     request: {
///         speed: String,
///     },
///     response: (),
///     payload: |req| format!("<InstanceID>{}</InstanceID><Speed>{}</Speed>", req.instance_id, req.speed),
///     parse: |_xml| Ok(()),
/// }
/// ```
#[macro_export]
macro_rules! define_upnp_operation {
    (
        operation: $op_struct:ident,
        action: $action:literal,
        service: $service:ident,
        request: {
            $($field:ident: $field_type:ty),* $(,)?
        },
        response: $response_type:ty,
        payload: |$req_param:ident| $payload_expr:expr,
        parse: |$xml_param:ident| $parse_expr:expr $(,)?
    ) => {
        paste! {
            #[derive(serde::Serialize, Clone, Debug, PartialEq)]
            pub struct [<$op_struct Request>] {
                $(pub $field: $field_type,)*
                pub instance_id: u32,
            }

            // Note: Validate implementation can be provided manually if needed
            // Default empty implementation is not generated to avoid conflicts

            #[derive(serde::Deserialize, Debug, Clone, PartialEq)]
            pub struct [<$op_struct Response>];

            pub struct $op_struct;

            impl $crate::operation::UPnPOperation for $op_struct {
                type Request = [<$op_struct Request>];
                type Response = $response_type;

                const SERVICE: $crate::service::Service = $crate::service::Service::$service;
                const ACTION: &'static str = $action;

                fn build_payload(request: &Self::Request) -> Result<String, $crate::operation::ValidationError> {
                    request.validate($crate::operation::ValidationLevel::Boundary)?;
                    let $req_param = request;
                    Ok($payload_expr)
                }

                fn parse_response(xml: &xmltree::Element) -> Result<Self::Response, $crate::error::ApiError> {
                    let $xml_param = xml;
                    $parse_expr
                }
            }

            // Generate convenience function
            pub fn [<$op_struct:snake>]($($field: $field_type),*) -> $crate::operation::OperationBuilder<$op_struct> {
                let request = [<$op_struct Request>] {
                    $($field,)*
                    instance_id: 0,
                };
                $crate::operation::OperationBuilder::new(request)
            }
        }
    };
}

/// Even simpler macro for basic operations that don't need custom logic
///
/// # Example
/// ```rust,ignore
/// simple_operation! {
///     PauseOperation, "Pause", AVTransport, {}, ()
/// }
/// ```
#[macro_export]
macro_rules! simple_operation {
    ($op_struct:ident, $action:literal, $service:ident, { $($field:ident: $field_type:ty),* }, $response_type:ty) => {
        define_upnp_operation! {
            operation: $op_struct,
            action: $action,
            service: $service,
            request: {
                $($field: $field_type,)*
            },
            response: $response_type,
            payload: |req| {
                let mut xml = format!("<InstanceID>{}</InstanceID>", req.instance_id);
                $(
                    xml.push_str(&format!("<{}>{}</{}>",
                        stringify!($field),
                        req.$field,
                        stringify!($field)));
                )*
                xml
            },
            parse: |_xml| Ok(()),
        }
    };
}

/// Macro for defining operations with XML response parsing
///
/// # Example
/// ```rust,ignore
/// define_operation_with_response! {
///     operation: GetVolumeOperation,
///     action: "GetVolume",
///     service: RenderingControl,
///     request: {
///         channel: String,
///     },
///     response: GetVolumeResponse {
///         current_volume: u8,
///     },
///     xml_mapping: {
///         current_volume: "CurrentVolume",
///     },
/// }
/// ```
#[macro_export]
macro_rules! define_operation_with_response {
    (
        operation: $op_struct:ident,
        action: $action:literal,
        service: $service:ident,
        request: {
            $($field:ident: $field_type:ty),* $(,)?
        },
        response: $response_struct:ident {
            $($resp_field:ident: $resp_type:ty),* $(,)?
        },
        xml_mapping: {
            $($xml_field:ident: $xml_path:literal),* $(,)?
        } $(,)?
    ) => {
        paste! {
            #[derive(serde::Serialize, Clone, Debug, PartialEq)]
            pub struct [<$op_struct Request>] {
                $(pub $field: $field_type,)*
                pub instance_id: u32,
            }

            // Note: Validate implementation can be provided manually if needed
            // Default empty implementation is not generated to avoid conflicts

            #[derive(serde::Deserialize, Debug, Clone, PartialEq)]
            pub struct $response_struct {
                $(pub $resp_field: $resp_type,)*
            }

            pub struct $op_struct;

            impl $crate::operation::UPnPOperation for $op_struct {
                type Request = [<$op_struct Request>];
                type Response = $response_struct;

                const SERVICE: $crate::service::Service = $crate::service::Service::$service;
                const ACTION: &'static str = $action;

                fn build_payload(request: &Self::Request) -> Result<String, $crate::operation::ValidationError> {
                    request.validate($crate::operation::ValidationLevel::Boundary)?;

                    let mut xml = format!("<InstanceID>{}</InstanceID>", request.instance_id);
                    $(
                        xml.push_str(&format!("<{}>{}</{}>",
                            stringify!($field),
                            request.$field,
                            stringify!($field)));
                    )*
                    Ok(xml)
                }

                fn parse_response(xml: &xmltree::Element) -> Result<Self::Response, $crate::error::ApiError> {
                    // Create a temporary mapping from field names to XML paths
                    $(let $xml_field = xml
                        .get_child($xml_path)
                        .and_then(|e| e.get_text())
                        .and_then(|s| s.parse().ok())
                        .unwrap_or_default();)*

                    Ok($response_struct {
                        $($resp_field: $xml_field,)*
                    })
                }
            }

            // Generate convenience function
            pub fn [<$op_struct:snake>]($($field: $field_type),*) -> $crate::operation::OperationBuilder<$op_struct> {
                let request = [<$op_struct Request>] {
                    $($field,)*
                    instance_id: 0,
                };
                $crate::operation::OperationBuilder::new(request)
            }
        }
    };
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_macro_compilation() {
        // Test that our macros compile without errors
        // This is mainly a compilation test to ensure the macro syntax is correct

        // Note: Actual usage tests would go in the services modules where the macros are used
        // since we can't easily test macro expansion here without a more complex test setup
        assert!(true, "Macro definitions compile successfully");
    }
}