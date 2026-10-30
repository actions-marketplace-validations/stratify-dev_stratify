def main
  puts "hi"
end

# stratify-ignore: dead_code
def kept_for_reflection
  1
end

NOTE = "stratify-ignore: dead_code"
def really_unused
  2
end

main
