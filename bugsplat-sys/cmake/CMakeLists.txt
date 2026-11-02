# Builds bugsplat-native for bugsplat-sys: the static SDK, BugSplatMonitor and BugSplatReporter.
#
# Cargo links one archive, so this wrapper walks the link closure of the `bugsplat` target
# (the core, Crashpad's client libraries, frameworks and system libraries), merges every static
# archive into libbugsplat_rs.a / bugsplat_rs.lib and writes the remaining system libraries to
# bugsplat-link.txt for build.rs to turn into cargo:rustc-link-lib lines.
cmake_minimum_required(VERSION 3.22)
project(bugsplat_rs LANGUAGES C CXX)

if(NOT BUGSPLAT_NATIVE_DIR)
  message(FATAL_ERROR "Pass -DBUGSPLAT_NATIVE_DIR=<bugsplat-native checkout>")
endif()

set(BUGSPLAT_STATIC ON CACHE BOOL "" FORCE)
set(BUGSPLAT_BUILD_TESTS OFF CACHE BOOL "" FORCE)
set(BUGSPLAT_BUILD_SAMPLES OFF CACHE BOOL "" FORCE)
add_subdirectory("${BUGSPLAT_NATIVE_DIR}" bugsplat-native)

# ---- link closure -------------------------------------------------------------------------
# Accumulated in global properties so the recursion needs no PARENT_SCOPE plumbing.
set_property(GLOBAL PROPERTY _BS_ARCHIVES "")
set_property(GLOBAL PROPERTY _BS_SYSTEM "")
set_property(GLOBAL PROPERTY _BS_SEEN "")

function(_bs_visit_item item)
  # Unwrap the generator expressions CMake puts around usage requirements.
  string(REGEX REPLACE "^\\$<LINK_ONLY:(.*)>$" "\\1" item "${item}")
  string(REGEX REPLACE "^\\$<BUILD_INTERFACE:(.*)>$" "\\1" item "${item}")
  if(item STREQUAL "" OR item MATCHES "^\\$<INSTALL_INTERFACE:")
    return()
  endif()
  if(item MATCHES "^\\$<")
    message(WARNING "bugsplat-sys: unhandled link item ${item}")
    return()
  endif()
  if(TARGET "${item}")
    get_target_property(_aliased "${item}" ALIASED_TARGET)
    if(_aliased)
      set(item "${_aliased}")
    endif()
    _bs_visit_target("${item}")
  elseif(item MATCHES "\\.(a|lib)$" AND EXISTS "${item}")
    set_property(GLOBAL APPEND PROPERTY _BS_ARCHIVES "${item}")
  else()
    # -framework X, bare library names (bsm, z, dl, winhttp, ...).
    set_property(GLOBAL APPEND PROPERTY _BS_SYSTEM "${item}")
  endif()
endfunction()

function(_bs_visit_target tgt)
  get_property(_seen GLOBAL PROPERTY _BS_SEEN)
  if("${tgt}" IN_LIST _seen)
    return()
  endif()
  set_property(GLOBAL APPEND PROPERTY _BS_SEEN "${tgt}")

  get_target_property(_type "${tgt}" TYPE)
  get_target_property(_imported "${tgt}" IMPORTED)
  if(_type STREQUAL "STATIC_LIBRARY")
    if(_imported)
      get_target_property(_loc "${tgt}" IMPORTED_LOCATION)
      if(NOT _loc)
        get_target_property(_loc "${tgt}" IMPORTED_LOCATION_RELEASE)
      endif()
      set_property(GLOBAL APPEND PROPERTY _BS_ARCHIVES "${_loc}")
    else()
      set_property(GLOBAL APPEND PROPERTY _BS_ARCHIVES "$<TARGET_LINKER_FILE:${tgt}>")
      get_target_property(_deps "${tgt}" LINK_LIBRARIES)
      if(_deps)
        foreach(_it IN LISTS _deps)
          _bs_visit_item("${_it}")
        endforeach()
      endif()
    endif()
  endif()
  get_target_property(_ideps "${tgt}" INTERFACE_LINK_LIBRARIES)
  if(_ideps)
    foreach(_it IN LISTS _ideps)
      _bs_visit_item("${_it}")
    endforeach()
  endif()
endfunction()

_bs_visit_target(bugsplat)
get_property(_bs_archives GLOBAL PROPERTY _BS_ARCHIVES)
get_property(_bs_system GLOBAL PROPERTY _BS_SYSTEM)
list(REMOVE_DUPLICATES _bs_archives)
list(REMOVE_DUPLICATES _bs_system)
message(STATUS "bugsplat-sys archives: ${_bs_archives}")
message(STATUS "bugsplat-sys system libs: ${_bs_system}")

# ---- one archive for cargo ----------------------------------------------------------------
if(MSVC)
  set(_bs_merged "${CMAKE_BINARY_DIR}/bugsplat_rs.lib")
  find_program(_bs_lib_exe lib.exe HINTS "${CMAKE_LINKER}/.." REQUIRED)
  set(_bs_merge_cmd "${_bs_lib_exe}" /NOLOGO "/OUT:${_bs_merged}" ${_bs_archives})
elseif(APPLE)
  set(_bs_merged "${CMAKE_BINARY_DIR}/libbugsplat_rs.a")
  set(_bs_merge_cmd /usr/bin/libtool -static -no_warning_for_no_symbols -o "${_bs_merged}" ${_bs_archives})
else()
  # GNU ar's MRI script merges archives member by member.
  set(_bs_merged "${CMAKE_BINARY_DIR}/libbugsplat_rs.a")
  set(_bs_mri "${CMAKE_BINARY_DIR}/bugsplat_rs.mri")
  set(_bs_mri_lines "CREATE ${_bs_merged}\n")
  foreach(_a IN LISTS _bs_archives)
    string(APPEND _bs_mri_lines "ADDLIB ${_a}\n")
  endforeach()
  string(APPEND _bs_mri_lines "SAVE\nEND\n")
  file(GENERATE OUTPUT "${_bs_mri}" CONTENT "${_bs_mri_lines}")
  set(_bs_merge_cmd sh -c "${CMAKE_AR} -M < '${_bs_mri}'")
endif()

add_custom_command(
  OUTPUT "${_bs_merged}"
  COMMAND ${CMAKE_COMMAND} -E rm -f "${_bs_merged}"
  COMMAND ${_bs_merge_cmd}
  DEPENDS bugsplat ${_bs_archives}
  COMMAND_EXPAND_LISTS
  VERBATIM)

set(_bs_helpers "")
if(TARGET BugSplatMonitor)
  list(APPEND _bs_helpers BugSplatMonitor)
endif()
if(TARGET BugSplatReporter)
  list(APPEND _bs_helpers BugSplatReporter)
endif()
add_custom_target(bugsplat_rs ALL DEPENDS "${_bs_merged}" ${_bs_helpers})

# build.rs reads this: one `key=value` per line.
string(REPLACE ";" "|" _bs_system_out "${_bs_system}")
set(_bs_info "archive=${_bs_merged}\nsystem=${_bs_system_out}\n")
if(TARGET BugSplatMonitor)
  string(APPEND _bs_info "monitor=$<TARGET_FILE:BugSplatMonitor>\n")
endif()
if(TARGET BugSplatReporter)
  if(APPLE)
    string(APPEND _bs_info "reporter=$<TARGET_BUNDLE_DIR:BugSplatReporter>\n")
  else()
    string(APPEND _bs_info "reporter=$<TARGET_FILE:BugSplatReporter>\n")
  endif()
  string(APPEND _bs_info "reporter_dir=$<TARGET_FILE_DIR:BugSplatReporter>\n")
endif()
file(GENERATE OUTPUT "${CMAKE_BINARY_DIR}/bugsplat-link.txt" CONTENT "${_bs_info}")
