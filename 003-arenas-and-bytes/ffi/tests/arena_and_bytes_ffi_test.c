#include <stdlib.h>
#include <stdio.h>
#include "../src/arena_and_bytes_ffi.h"

int main(int argv, char** argc) {
    span_t span = {
            .ptr = 0,
            .len = 0
    };
    field_id_t field = 1;
    malloc_function_t my_malloc = malloc;

    error_code_t ec = rust_read_dynamically_sized_field_c(field, my_malloc, &span);
    if (ec) {
        fprintf(stderr,"ERROR: %d\n", ec);
        return ec;
    }


    printf("rust: %s\n", (const char*) span.ptr);

    ProxyAllocator allocator = {
            .alloc = rust_alloc_aligned,
            .free = rust_free,
    };

    uint64_t* heap_num = (uint64_t*)Allocator_alloc(&allocator, sizeof(uint64_t), sizeof(uint64_t));
    *heap_num = 4314lu;
    fprintf(stdout, "heap_num: %lu", *heap_num);
    Allocator_free(&allocator, (void*) heap_num, sizeof(uint64_t), sizeof(uint64_t));

    return 0;
}