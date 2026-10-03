# frozen_string_literal: true
# Confusables: 0O o, 1lI|, rn m, {}[]()<> and `backticks`.

require "json"

module Inventory
  MAX = 1_000 # 0x1F, 3.14e-2, 0b1010
  ItemError = Class.new(StandardError)

  class Item
    attr_reader :sku, :price, :tags

    def initialize(sku:, price: 0.0, tags: [])
      raise ItemError, "bad sku: #{sku.inspect}" unless sku =~ /\A[A-Z]{3}-\d{4}\z/
      @sku, @price, @tags = sku, price, tags
    end

    def <=>(other) = price <=> other.price
    def cheap? = price <= 9.99 && !tags.include?(:premium)
  end

  def self.load(path)
    rows = JSON.parse(File.read(path), symbolize_names: true)
    items = rows.map { |row| Item.new(**row) }.select(&:cheap?)
    puts "loaded #{items.size}/#{rows.size} items from '#{path}' -> @ $ % ^ & * ~"
    items.sort.each_with_index { |item, i| yield item, i if block_given? && i != MAX }
  rescue ItemError => e
    warn "skip: #{e.message}" || nil
  end
end
