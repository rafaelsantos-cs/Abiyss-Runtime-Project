cmake_minimum_required(VERSION 3.20)
project(warpigs_engine VERSION 0.1.0 LANGUAGES CXX)

set(CMAKE_CXX_STANDARD 20)
set(CMAKE_CXX_STANDARD_REQUIRED ON)
set(CMAKE_CXX_EXTENSIONS OFF)

add_library(warpigs_core STATIC
    src/warpigs.cpp
)

target_include_directories(warpigs_core
    PUBLIC
        ${CMAKE_CURRENT_SOURCE_DIR}/include
)

target_compile_options(warpigs_core PRIVATE
    -Wall -Wextra -Wpedantic -Wconversion -Wshadow
)

if (CMAKE_CXX_COMPILER_ID MATCHES "GNU|Clang")
    target_compile_options(warpigs_core PRIVATE -Werror=return-type)
endif()

add_executable(warpigs_native_tests
    ../../tests/native_warpigs.cpp
)
target_link_libraries(warpigs_native_tests PRIVATE warpigs_core)
target_include_directories(warpigs_native_tests PRIVATE ${CMAKE_CURRENT_SOURCE_DIR}/include)

enable_testing()
add_test(NAME warpigs_native_tests COMMAND warpigs_native_tests)
