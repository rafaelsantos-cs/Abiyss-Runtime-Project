cmake_minimum_required(VERSION 3.20)
project(abiyss_system LANGUAGES CXX)

set(CMAKE_CXX_STANDARD 20)
set(CMAKE_CXX_STANDARD_REQUIRED ON)
set(CMAKE_CXX_EXTENSIONS OFF)

add_executable(abiyss-system main.cpp)
target_compile_options(abiyss-system PRIVATE -Wall -Wextra -Wpedantic)
