CMAKE_MINIMUM_REQUIRED (VERSION 3.1)
PROJECT (ArenasAndBytesFfi)

SET(CMAKE_C_COMPILER /usr/bin/clang-7)
SET(CMAKE_CXX_COMPILER /usr/bin/clang++-7)
SET (ArenaAndBytesFfi_VERSION_MAJOR 1)
SET (ArenaAndBytesFfi_VERSION_MINOR 0)


INCLUDE_DIRECTORIES(${CMAKE_SOURCE_DIR}/src)

#ADD_SUBDIRECTORY(tests)
#INCLUDE_DIRECTORIES(tests)
SET(CMAKE_LIBRARY_PATH   "$ENV{LD_LIBRARY_PATH}")

ADD_EXECUTABLE(ArenaAndBytesTest ${CMAKE_SOURCE_DIR}/tests/arena_and_bytes_ffi_test.c)
#ADD_LINK_OPTIONS(ArenaAndBytesTest "-L${CMAKE_LIBRARY_PATH}")
TARGET_LINK_LIBRARIES(ArenaAndBytesTest -L${CMAKE_LIBRARY_PATH} potatofarmer)
# Indicate which libraries to include during the link process.
#target_link_libraries (CMakeHelloWorld Hello)

#install (TARGETS CMakeHelloWorld DESTINATION bin)