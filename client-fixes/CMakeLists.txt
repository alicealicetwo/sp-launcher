cmake_minimum_required(VERSION 3.21)
project(SPClientFixes LANGUAGES CXX)

add_library(SPClientFixes SHARED src/client_fixes.cpp)
target_compile_features(SPClientFixes PRIVATE cxx_std_20)
target_compile_definitions(SPClientFixes PRIVATE WIN32_LEAN_AND_MEAN NOMINMAX)
set_target_properties(SPClientFixes PROPERTIES OUTPUT_NAME SPClientFixes)

if(NOT WIN32 OR NOT CMAKE_SIZEOF_VOID_P EQUAL 8)
    message(FATAL_ERROR "SPClientFixes requires a 64-bit Windows toolchain")
endif()
