#![recursion_limit = "256"]
use criterion::{black_box, criterion_group, criterion_main, Criterion};
use generic_array1::{arr, ArrayLength};
use struthionidae::{self, byte_arrays::ByteArray};

fn bytes_to_hex_string(bytes: &[u8]) -> String {
    struthionidae::bytes::to_lower_hex_string(bytes)
}

fn bytes_to_hexstr_to_string(bytes: &[u8]) -> String {
    struthionidae::bytes::HexStr::from(bytes).to_string()
}

fn bytes_to_hex_string2(bytes: &[u8]) -> String {
    let mut string = String::with_capacity(bytes.len() * 2);
    for b in bytes {
        string.push_str(struthionidae::bytes::to_lower_hex(*b));
    }
    string
}

fn byte_array_to_hex_string_array<N>(b: &ByteArray<N>) -> String
where
    N: ArrayLength<u8> + ArrayLength<&'static str>,
{
    struthionidae::byte_arrays::to_lower_hex_array(b).join("")
}

fn bytes_array_to_hex_string_direct<N>(b: &ByteArray<N>) -> String
where
    N: ArrayLength<u8> + ArrayLength<&'static str>,
{
    struthionidae::byte_arrays::to_lower_hex_string(b)
}

fn criterion_benchmark(c: &mut Criterion) {
    let input = arr![u8; 132, 0, 6, 101, 62, 154, 201, 233, 81, 23, 161, 92, 145, 92, 170, 184, 22, 98, 145, 142, 146, 93, 233, 224, 4, 247, 116, 255, 130, 215, 7, 154, 64, 212, 210, 123, 27, 55, 38, 87, 198, 29, 70, 212, 112, 48, 76, 136, 199, 136, 179, 164, 82, 122, 208, 116, 209, 220, 203, 238, 93, 186, 169, 154];

    c.bench_function("bytes_to_hex_string", |b| {
        b.iter(|| bytes_to_hex_string(black_box(&input[..])))
    });

    c.bench_function("bytes_to_hex_string2", |b| {
        b.iter(|| bytes_to_hex_string2(black_box(&input[..])))
    });
    c.bench_function("bytes_to_hexstr_to_string", |b| {
        b.iter(|| bytes_to_hexstr_to_string(black_box(&input[..])))
    });
    c.bench_function("byte_array_to_hex_string_array", |b| {
        b.iter(|| byte_array_to_hex_string_array(black_box(&input)))
    });
    c.bench_function("BIG_byte_array_to_hex_string_direct", |b| {
        b.iter(|| bytes_array_to_hex_string_direct(black_box(&input)))
    });

    let slice: &[u8] = include!("biginput.array");
    let input = generic_array1::GenericArray::<_, generic_array1::typenum::U8192>::from_slice(slice);

    c.bench_function("BIG_bytes_to_hex_string", |b| {
        b.iter(|| bytes_to_hex_string(black_box(&input[..])))
    });

    c.bench_function("BIG_bytes_to_hex_string2", |b| {
        b.iter(|| bytes_to_hex_string2(black_box(&input[..])))
    });
    c.bench_function("BIG_bytes_to_hexstr_to_string", |b| {
        b.iter(|| bytes_to_hexstr_to_string(black_box(&input[..])))
    });
    c.bench_function("BIG_byte_array_to_hex_string_array", |b| {
        b.iter(|| byte_array_to_hex_string_array(black_box(input)))
    });

    c.bench_function("BIG_byte_array_to_hex_string_direct", |b| {
        b.iter(|| bytes_array_to_hex_string_direct(black_box(input)))
    });
}

criterion_group!(benches, criterion_benchmark);
criterion_main!(benches);
