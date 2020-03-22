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
typedef void* (*AlignedAllocFn)(size_t, size_t);
typedef void (*FreeFn)(void*, size_t, size_t);


typedef struct span {
    void* ptr;
    size_t len;
} span_t;


error_code_t
rust_read_dynamically_sized_field_c(field_id_t field_id, malloc_function_t malloc_function, span_t* span);


typedef struct _ProxyAllocator {
    AlignedAllocFn alloc;
    FreeFn free;
} ProxyAllocator;

void* rust_alloc_aligned(size_t,size_t);
void rust_free(void*, size_t, size_t);

void* Allocator_alloc(const ProxyAllocator*, size_t, size_t);
void Allocator_free(const ProxyAllocator*, void*, size_t, size_t);


#ifdef __cplusplus
}
#endif /* __cplusplus */

#endif /* ARENA_AND_BYTES_FFI_H */