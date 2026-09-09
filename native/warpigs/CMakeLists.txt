cmake_minimum_required(VERSION 3.20)
project(warpigs_engine VERSION 0.1.0 LANGUAGES CXX)

set(CMAKE_CXX_STANDARD 20)
set(CMAKE_CXX_STANDARD_REQUIRED ON)
set(CMAKE_CXX_EXTENSIONS OFF)

set(WARPIGS_WARNINGS
    -Wall -Wextra -Wpedantic -Wconversion -Wshadow
)

add_library(warpigs_core STATIC
    src/warpigs.cpp
)

add_library(warpigs_core_shared SHARED
    src/warpigs.cpp
)

set_target_properties(warpigs_core_shared PROPERTIES OUTPUT_NAME warpigs_core)

foreach(target warpigs_core warpigs_core_shared)
    target_include_directories(${target}
        PUBLIC
            ${CMAKE_CURRENT_SOURCE_DIR}/include
    )
    target_compile_options(${target} PRIVATE ${WARPIGS_WARNINGS})
    if (CMAKE_CXX_COMPILER_ID MATCHES "GNU|Clang")
        target_compile_options(${target} PRIVATE -Werror=return-type)
    endif()
endforeach()

add_executable(warpigs_native_tests
    ../../tests/native_warpigs.cpp
)
target_link_libraries(warpigs_native_tests PRIVATE warpigs_core)
target_include_directories(warpigs_native_tests PRIVATE ${CMAKE_CURRENT_SOURCE_DIR}/include)
target_compile_options(warpigs_native_tests PRIVATE ${WARPIGS_WARNINGS})

add_executable(warpigs_hostile_tests
    ../../tests/hostile_warpigs.cpp
)
target_link_libraries(warpigs_hostile_tests PRIVATE warpigs_core)
target_include_directories(warpigs_hostile_tests PRIVATE ${CMAKE_CURRENT_SOURCE_DIR}/include)
target_compile_options(warpigs_hostile_tests PRIVATE ${WARPIGS_WARNINGS})

include(GNUInstallDirs)
install(TARGETS warpigs_core_shared
    LIBRARY DESTINATION ${CMAKE_INSTALL_LIBDIR}
    RUNTIME DESTINATION ${CMAKE_INSTALL_BINDIR}
    ARCHIVE DESTINATION ${CMAKE_INSTALL_LIBDIR}
)
install(FILES include/warpigs.h DESTINATION ${CMAKE_INSTALL_INCLUDEDIR}/warpigs)

enable_testing()
add_test(NAME warpigs_native_tests COMMAND warpigs_native_tests)
add_test(NAME warpigs_hostile_tests COMMAND warpigs_hostile_tests)
