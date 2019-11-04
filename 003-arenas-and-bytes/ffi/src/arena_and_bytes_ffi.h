#ifndef ARENA_AND_BYTES_FFI_H
#define ARENA_AND_BYTES_FFI_H

#ifdef __cplusplus
extern "C" {
#endif /* __cplusplus */

#include <stddef.h>
#include <stdint.h>

typedef int32_t error_code_t;
typedef int32_t field_id_t;
typedef void* (*malloc_function_t)(size_t);

typedef struct span {
    void* ptr;
    size_t len;
} span_t;


error_code_t
rust_read_dynamically_sized_field_c(field_id_t field_id, malloc_function_t malloc_function, span_t* span);

#ifdef __cplusplus
}
#endif /* __cplusplus */

#endif /* ARENA_AND_BYTES_FFI_H */