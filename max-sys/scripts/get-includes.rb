#!/usr/bin/env ruby

base = ARGV[0]
raise "#{base} is not a directory" unless File.directory?(base)

max = "max-includes"
msp = "msp-includes"
jitter = "jit-includes"

[max, msp, jitter].each do |d|
  d = File.join(base, d)
  raise "expected #{d} to be a directory" unless File.directory?(d)
end

INCLUDE_REGEX = /\A\s*#include\s*(?:"|<)(.*)(?:"|>)/

def get_includes(dir, common_header)
  common = []
  File.readlines(File.join(dir, common_header)).each do |l|
    m = INCLUDE_REGEX.match(l)
    common << m[1] if m
  end
  headers = []
  Dir.glob(File.join(dir, "*.h")).each do |h|
    h = File.basename(h)
    headers << h unless common.include?(h)
  end
  headers.sort!
  headers.delete(common_header)
  headers.unshift(common_header)
  headers
end

def write_includes(filename, headers)
  File.open(filename, "w") do |f|
    headers.each do |h|
      f.puts "#include <#{h}>"
    end
  end
end

#get max headers
headers = get_includes(File.join(base, max), "ext.h")
%w(ext.h jgraphics.h).reverse.each do |h|
  headers.delete(h)
  headers.unshift(h)
end
write_includes("wrapper-max.h", headers)

headers = get_includes(File.join(base, jitter), "jit.common.h")
write_includes("wrapper-jitter.h", headers)
