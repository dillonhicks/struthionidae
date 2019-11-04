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

    return 0;
}