# 1.2.0

- Actually, there is no need to extract anything to a temporary directory. This can just read a stream from the zip directly

# 1.1.0

- Only extract relevant class files instead of extracting everything and then running glob. Might be faster. Or not, who knows
