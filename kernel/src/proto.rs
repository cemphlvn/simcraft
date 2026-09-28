//! The part of `onnx.proto` the kernel reads and writes, declared by hand (no `protoc` in the build).
//! Tags follow https://github.com/onnx/onnx/blob/main/onnx/onnx.proto; fields we do not use are skipped on decode.

#[derive(Clone, PartialEq, prost::Message)]
pub struct ModelProto {
    #[prost(int64, tag = "1")]
    pub ir_version: i64,
    #[prost(string, tag = "2")]
    pub producer_name: String,
    #[prost(message, optional, tag = "7")]
    pub graph: Option<GraphProto>,
    #[prost(message, repeated, tag = "8")]
    pub opset_import: Vec<OperatorSetIdProto>,
}

#[derive(Clone, PartialEq, prost::Message)]
pub struct OperatorSetIdProto {
    #[prost(string, tag = "1")]
    pub domain: String,
    #[prost(int64, tag = "2")]
    pub version: i64,
}

#[derive(Clone, PartialEq, prost::Message)]
pub struct GraphProto {
    #[prost(message, repeated, tag = "1")]
    pub node: Vec<NodeProto>,
    #[prost(string, tag = "2")]
    pub name: String,
    #[prost(message, repeated, tag = "5")]
    pub initializer: Vec<TensorProto>,
    #[prost(message, repeated, tag = "11")]
    pub input: Vec<ValueInfoProto>,
    #[prost(message, repeated, tag = "12")]
    pub output: Vec<ValueInfoProto>,
}

#[derive(Clone, PartialEq, prost::Message)]
pub struct NodeProto {
    #[prost(string, repeated, tag = "1")]
    pub input: Vec<String>,
    #[prost(string, repeated, tag = "2")]
    pub output: Vec<String>,
    #[prost(string, tag = "3")]
    pub name: String,
    #[prost(string, tag = "4")]
    pub op_type: String,
    #[prost(message, repeated, tag = "5")]
    pub attribute: Vec<AttributeProto>,
    #[prost(string, tag = "7")]
    pub domain: String,
}

#[derive(Clone, PartialEq, prost::Message)]
pub struct AttributeProto {
    #[prost(string, tag = "1")]
    pub name: String,
    #[prost(float, tag = "2")]
    pub f: f32,
    #[prost(int64, tag = "3")]
    pub i: i64,
    #[prost(bytes = "vec", tag = "4")]
    pub s: Vec<u8>,
    #[prost(message, optional, tag = "5")]
    pub t: Option<TensorProto>,
    #[prost(float, repeated, tag = "7")]
    pub floats: Vec<f32>,
    #[prost(int64, repeated, packed = "false", tag = "8")]
    pub ints: Vec<i64>,
    /// AttributeType: FLOAT 1, INT 2, STRING 3, TENSOR 4, FLOATS 6, INTS 7.
    #[prost(int32, tag = "20")]
    pub r#type: i32,
}

#[derive(Clone, PartialEq, prost::Message)]
pub struct TensorProto {
    #[prost(int64, repeated, packed = "false", tag = "1")]
    pub dims: Vec<i64>,
    /// TensorProto.DataType (see `DType::from_onnx`).
    #[prost(int32, tag = "2")]
    pub data_type: i32,
    #[prost(float, repeated, tag = "4")]
    pub float_data: Vec<f32>,
    /// INT32, and the narrow types (INT8, UINT8, INT16, UINT16, BOOL) one value per entry.
    #[prost(int32, repeated, tag = "5")]
    pub int32_data: Vec<i32>,
    #[prost(int64, repeated, tag = "7")]
    pub int64_data: Vec<i64>,
    #[prost(string, tag = "8")]
    pub name: String,
    /// Little-endian values at the type's own width.
    #[prost(bytes = "vec", tag = "9")]
    pub raw_data: Vec<u8>,
    /// 1 = EXTERNAL (weights in another file): not supported, a brain is one file.
    #[prost(int32, tag = "14")]
    pub data_location: i32,
}

#[derive(Clone, PartialEq, prost::Message)]
pub struct ValueInfoProto {
    #[prost(string, tag = "1")]
    pub name: String,
    #[prost(message, optional, tag = "2")]
    pub r#type: Option<TypeProto>,
}

#[derive(Clone, PartialEq, prost::Message)]
pub struct TypeProto {
    #[prost(message, optional, tag = "1")]
    pub tensor_type: Option<TypeTensor>,
}

#[derive(Clone, PartialEq, prost::Message)]
pub struct TypeTensor {
    #[prost(int32, tag = "1")]
    pub elem_type: i32,
    #[prost(message, optional, tag = "2")]
    pub shape: Option<TensorShapeProto>,
}

#[derive(Clone, PartialEq, prost::Message)]
pub struct TensorShapeProto {
    #[prost(message, repeated, tag = "1")]
    pub dim: Vec<Dimension>,
}

#[derive(Clone, PartialEq, prost::Message)]
pub struct Dimension {
    #[prost(int64, optional, tag = "1")]
    pub dim_value: Option<i64>,
    /// A named dimension (e.g. "batch"): any size.
    #[prost(string, optional, tag = "2")]
    pub dim_param: Option<String>,
}
